//! A save partway through a read must leave the active tuning in place.

use beryl_host::DataWatch;
use kit_base::data::Data;

#[test]
fn a_save_between_file_reads_never_publishes_the_mixed_set() {
    let mut files = [1, 1];
    let mut reads = 0;
    let read = move || {
        let first = files[0];
        reads += 1;
        if reads == 3 {
            files = [2, 2];
        }
        Ok([first, files[1]])
    };
    let mut watch = DataWatch::new(read, |&[first, second]| {
        let mut data = Data::clone(&crate::test_data::data());
        data.tier_probe.frames = first * 10 + second;
        Ok(data)
    });

    assert_eq!(watch.poll().unwrap().unwrap().tier_probe.frames, 11);
    assert!(watch.poll().is_none(), "the first read saw [1, 2]");
    assert_eq!(watch.poll().unwrap().unwrap().tier_probe.frames, 22);
    assert!(watch.poll().is_none());
}
