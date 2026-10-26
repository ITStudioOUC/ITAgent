//! 协议版本与版本协商。
//!
//! 协议是 server 与 worker 之间的单一 IPC 契约，不是库，所以版本只用一个主版本号表达，
//! 不用 semver 三元组。协商是纯函数：请求版本落在支持范围内则接受，否则拒绝并回显支持
//! 范围，让调用方看见错在哪。支持范围是可调参数，默认值由 [`DEFAULT_SUPPORTED_RANGE`]
//! 给出，server 启动时从配置结构读取，允许按项目覆盖，不散落在调用点。

use std::fmt;

/// 协议版本名义类型。
///
/// 协议是单一 IPC 契约，不是库，所以版本只用一个主版本号表达，不用 semver 三元组。名义
/// 类型拦住"把 crate 版本当协议版本传"这类错误：`ProtocolVersion` 与 `u32` 不可互换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(u32);

impl ProtocolVersion {
    /// 从主版本号构造。
    ///
    /// # 边界与前提
    /// 主版本号是 `u32`，任何值都合法，无校验。
    ///
    /// # 副作用
    /// 无。
    pub const fn new(major: u32) -> Self {
        Self(major)
    }

    /// 取主版本号。
    pub const fn major(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 当前实现的协议版本，信封的 `version` 字段写这个值。
pub const CURRENT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(1);

/// 支持范围构造失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvertedRange {
    /// 构造时给出的下限。
    pub min: ProtocolVersion,
    /// 构造时给出的上限。
    pub max: ProtocolVersion,
}

impl fmt::Display for InvertedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "支持范围下限 {} 大于上限 {}", self.min, self.max)
    }
}

impl std::error::Error for InvertedRange {}

/// 一个 server 实现的协议版本支持范围，闭区间 `min..=max`。
///
/// 这是可调参数：默认值由 [`DEFAULT_SUPPORTED_RANGE`] 给出，server 启动时从配置结构读取，
/// 允许按项目、按工作区覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedRange {
    /// 接受的下限（含）。
    pub min: ProtocolVersion,
    /// 接受的上限（含）。
    pub max: ProtocolVersion,
}

impl SupportedRange {
    /// 构造支持范围，下限大于上限被拒。
    ///
    /// # 边界与前提
    /// - 下限等于上限是合法的单版本范围。
    /// - 下限大于上限被拒，错误里带回两个值，调用方能看见错在哪。
    ///
    /// # 副作用
    /// 无。
    pub fn new(min: ProtocolVersion, max: ProtocolVersion) -> Result<Self, InvertedRange> {
        if min > max {
            return Err(InvertedRange { min, max });
        }
        Ok(Self { min, max })
    }
}

/// 默认支持范围：只接受当前实现的协议版本。
pub const DEFAULT_SUPPORTED_RANGE: SupportedRange =
    SupportedRange { min: CURRENT_PROTOCOL_VERSION, max: CURRENT_PROTOCOL_VERSION };

/// 协商接受的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accepted;

/// 协商拒绝的结果，回显支持范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejected {
    /// 被拒绝时 server 实际支持的范围，回显给调用方。
    pub supported: SupportedRange,
}

/// 版本协商：请求版本落在支持范围内则接受，否则拒绝并回显支持范围。
///
/// # 边界与前提
/// - 闭区间：`min` 与 `max` 本身被接受，`min-1` 与 `max+1` 被拒。
/// - 纯函数，同样的输入必然得到同样的结果。
///
/// # 副作用
/// 无。
pub fn negotiate(
    requested: ProtocolVersion,
    supported: SupportedRange,
) -> Result<Accepted, Rejected> {
    if requested >= supported.min && requested <= supported.max {
        Ok(Accepted)
    } else {
        Err(Rejected { supported })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 区间外的版本被拒，拒绝结果里回显支持范围，调用方看得见错在哪。
    #[test]
    fn incompatible_version_is_rejected_with_supported_range() {
        let supported =
            SupportedRange::new(ProtocolVersion::new(1), ProtocolVersion::new(4)).expect("合法");
        let err = negotiate(ProtocolVersion::new(5), supported).expect_err("5 不在 1..=4");
        assert_eq!(err, Rejected { supported });
        assert_eq!(err.supported.min, ProtocolVersion::new(1));
        assert_eq!(err.supported.max, ProtocolVersion::new(4));
    }

    /// 区间内的版本被接受。
    #[test]
    fn compatible_version_is_accepted() {
        let supported =
            SupportedRange::new(ProtocolVersion::new(1), ProtocolVersion::new(4)).expect("合法");
        assert_eq!(negotiate(ProtocolVersion::new(3), supported), Ok(Accepted));
    }

    /// 边界值 `min` 与 `max` 本身被接受，闭区间不是开区间。
    #[test]
    fn boundary_versions_min_and_max_are_accepted() {
        let supported =
            SupportedRange::new(ProtocolVersion::new(2), ProtocolVersion::new(5)).expect("合法");
        assert_eq!(negotiate(ProtocolVersion::new(2), supported), Ok(Accepted));
        assert_eq!(negotiate(ProtocolVersion::new(5), supported), Ok(Accepted));
    }

    /// `min-1` 与 `max+1` 被拒，区间外的 off-by-one 版本不会漏进。
    #[test]
    fn versions_outside_bounds_are_rejected() {
        let supported =
            SupportedRange::new(ProtocolVersion::new(2), ProtocolVersion::new(5)).expect("合法");
        assert!(negotiate(ProtocolVersion::new(1), supported).is_err());
        assert!(negotiate(ProtocolVersion::new(6), supported).is_err());
    }

    /// 下限大于上限的范围在构造期被拒，错误里带回两个值。
    #[test]
    fn inverted_range_is_rejected_at_construction() {
        let err = SupportedRange::new(ProtocolVersion::new(4), ProtocolVersion::new(1))
            .expect_err("下限大于上限应当被拒");
        assert_eq!(
            err,
            InvertedRange { min: ProtocolVersion::new(4), max: ProtocolVersion::new(1) }
        );
    }

    /// 下限等于上限是合法的单版本范围，默认范围就是这种形状。
    #[test]
    fn single_version_range_is_legal_and_default_is_one() {
        let single =
            SupportedRange::new(ProtocolVersion::new(3), ProtocolVersion::new(3)).expect("合法");
        assert_eq!(negotiate(ProtocolVersion::new(3), single), Ok(Accepted));
        assert_eq!(
            DEFAULT_SUPPORTED_RANGE,
            SupportedRange { min: CURRENT_PROTOCOL_VERSION, max: CURRENT_PROTOCOL_VERSION }
        );
    }
}
