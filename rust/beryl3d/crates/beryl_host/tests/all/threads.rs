//! Each scheduling class the app asks for is the one the thread then has, on the platforms that
//! have classes (macOS and Windows here; iOS shares the macOS code, and Android sets the nice
//! values `Class::nice` gives).

use beryl_host::{Class, current_class, set_class};

#[test]
fn a_thread_is_in_the_class_it_asked_for() {
    if cfg!(not(any(target_vendor = "apple", windows, target_os = "android"))) {
        return;
    }
    for class in [
        Class::Utility,
        Class::Default,
        Class::Initiated,
        Class::Interactive,
    ] {
        let found = std::thread::spawn(move || {
            set_class(class);
            current_class()
        })
        .join()
        .expect("the thread ends");
        assert_eq!(found, Some(class));
    }
}

/// Android runs a lower nice value first, so each class above another has a lower one, and the
/// AI's class is the nice value every thread starts at.
#[test]
fn android_nice_values_fall_as_the_class_rises() {
    let nice = [
        Class::Utility,
        Class::Default,
        Class::Initiated,
        Class::Interactive,
    ]
    .map(Class::nice);
    assert_eq!(nice, [10, 0, -2, -4]);
}
