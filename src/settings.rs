//! 设置：总开关、分组开关、几条默认关闭的噪音规则、方法行数上限。
//!
//! 诊断 provider 里可以读设置：宿主的 `settings::get` 不设能力门槛，
//! 也不要求交互式调用。每份文档只读一次，缓存在 `Ctx` 里。

use openidea_plugin::host;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Constant,
    Naming,
    Concurrent,
    Collection,
    Flow,
    Oop,
    Comment,
    Other,
    Exception,
}

impl Group {
    /// 注释组没有总开关：两条规则各自默认关闭，由自己的开关决定。
    fn key(self) -> Option<&'static str> {
        match self {
            Self::Constant => Some("group.constant"),
            Self::Naming => Some("group.naming"),
            Self::Concurrent => Some("group.concurrent"),
            Self::Collection => Some("group.collection"),
            Self::Flow => Some("group.flow"),
            Self::Oop => Some("group.oop"),
            Self::Comment => None,
            Self::Other => Some("group.other"),
            Self::Exception => Some("group.exception"),
        }
    }
}

pub struct Settings {
    pub enabled: bool,
    groups: [bool; 9],
    pub abstract_method_javadoc: bool,
    pub enum_constant_javadoc: bool,
    pub class_must_have_author: bool,
    pub collection_init_capacity: bool,
    pub max_method_lines: u32,
}

const GROUPS: [Group; 9] = [
    Group::Constant,
    Group::Naming,
    Group::Concurrent,
    Group::Collection,
    Group::Flow,
    Group::Oop,
    Group::Comment,
    Group::Other,
    Group::Exception,
];

impl Settings {
    pub fn load() -> Self {
        Self {
            enabled: host::setting_bool("enabled").unwrap_or(true),
            groups: std::array::from_fn(|i| GROUPS[i].key().is_none_or(|k| host::setting_bool(k).unwrap_or(true))),
            abstract_method_javadoc: host::setting_bool("comment.abstractMethodJavadoc").unwrap_or(false),
            enum_constant_javadoc: host::setting_bool("comment.enumConstantJavadoc").unwrap_or(false),
            class_must_have_author: host::setting_bool("comment.classMustHaveAuthor").unwrap_or(false),
            collection_init_capacity: host::setting_bool("collection.initCapacity").unwrap_or(false),
            max_method_lines: (host::setting_number("maxMethodLines").unwrap_or(80.0).max(1.0)) as u32,
        }
    }

    pub fn allows(&self, group: Group) -> bool {
        self.enabled && self.groups[group as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn defaults_keep_everything_on_except_the_comment_rules() {
        let _mock = MockHost::install();
        let s = Settings::load();
        assert!(s.enabled);
        assert_eq!(s.max_method_lines, 80);
        assert!(!s.abstract_method_javadoc);
        assert!(!s.enum_constant_javadoc);
        for g in GROUPS {
            assert!(s.allows(g), "{g:?}");
        }
    }

    #[test]
    fn the_master_switch_overrides_every_group() {
        let mock = MockHost::install();
        mock.set_setting("enabled", "false");
        let s = Settings::load();
        for g in GROUPS {
            assert!(!s.allows(g), "{g:?}");
        }
    }

    #[test]
    fn one_group_can_be_turned_off_alone() {
        let mock = MockHost::install();
        mock.set_setting("group.naming", "false");
        let s = Settings::load();
        assert!(!s.allows(Group::Naming));
        assert!(s.allows(Group::Flow));
        assert!(s.allows(Group::Comment));
    }

    #[test]
    fn the_comment_rules_have_their_own_switches() {
        let mock = MockHost::install();
        mock.set_setting("comment.abstractMethodJavadoc", "true");
        mock.set_setting("comment.enumConstantJavadoc", "true");
        let s = Settings::load();
        assert!(s.abstract_method_javadoc);
        assert!(s.enum_constant_javadoc);
    }

    #[test]
    fn the_method_line_limit_is_configurable_and_never_zero() {
        let mock = MockHost::install();
        mock.set_setting("maxMethodLines", "120");
        assert_eq!(Settings::load().max_method_lines, 120);
        mock.set_setting("maxMethodLines", "0");
        assert_eq!(Settings::load().max_method_lines, 1);
    }
}
