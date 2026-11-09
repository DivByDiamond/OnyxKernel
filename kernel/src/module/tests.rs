use super::*;

#[test]
fn test_module_type_as_str() {
    assert_eq!(ModuleType::Driver.as_str(), "driver");
    assert_eq!(ModuleType::FileSystem.as_str(), "fs");
    assert_eq!(ModuleType::Misc.as_str(), "misc");
}

#[test]
fn test_register_and_count() {
    unsafe {
        let before = count();
        register("test_mod", ModuleType::Misc);
        assert_eq!(count(), before + 1);
        unregister("test_mod");
        assert_eq!(count(), before);
    }
}

#[test]
fn test_find_by_name() {
    unsafe {
        register("find_me", ModuleType::Driver);
        let m = find_by_name("find_me");
        assert!(m.is_some());
        assert_eq!(m.unwrap().name, "find_me");
        assert_eq!(m.unwrap().type_, ModuleType::Driver);
        unregister("find_me");
    }
}

#[test]
fn test_unregister_nonexistent() {
    unsafe {
        assert!(!unregister("does_not_exist"));
    }
}

#[test]
fn test_format_list() {
    unsafe {
        register("fmt_test", ModuleType::FileSystem);
        let mut buf = [0u8; 256];
        let len = format_list(&mut buf);
        let s = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(s.contains("fmt_test"));
        assert!(s.contains("fs"));
        unregister("fmt_test");
    }
}

#[test]
fn test_module_type_equality() {
    assert_eq!(ModuleType::Driver, ModuleType::Driver);
    assert_ne!(ModuleType::Driver, ModuleType::FileSystem);
}

#[test]
fn test_module_not_zero_sized() {
    assert!(core::mem::size_of::<Module>() > 0);
}
