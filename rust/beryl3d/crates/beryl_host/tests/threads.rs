//! Each scheduling class the app asks for is the one the thread then has, on the platforms that
//! have classes (macOS and Windows here; iOS shares the macOS code).

use beryl_host::{Class, current_class, set_class};

#[test]
fn a_thread_is_in_the_class_it_asked_for() {
    if cfg!(not(any(target_vendor = "apple", windows))) {
        return;
    }
    for class in [Class::Utility, Class::Default, Class::Interactive] {
        let found = std::thread::spawn(move || {
            set_class(class);
            current_class()
        })
        .join()
        .expect("the thread ends");
        assert_eq!(found, Some(class));
    }
}
