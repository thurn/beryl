use super::*;

/// A dependency file as Cargo writes it: each path's length in four little-endian bytes, then
/// the path, with a kind byte before each.
fn encoded(paths: &[&str]) -> Vec<u8> {
    let mut out = u32::try_from(paths.len()).unwrap().to_le_bytes().to_vec();
    for path in paths {
        out.push(2);
        out.extend_from_slice(&u32::try_from(path.len()).unwrap().to_le_bytes());
        out.extend_from_slice(path.as_bytes());
    }
    out
}

#[test]
fn a_path_under_the_donor_moves_to_the_tree_with_its_length() {
    let data = encoded(&["src/lib.rs", "/w/tree-03/data/store.tsv", "/w/tree-030/x"]);
    let new = rewrite_dep_info(&data, b"/w/tree-03/", b"/w/landing-1/").unwrap();
    assert_eq!(
        new,
        encoded(&["src/lib.rs", "/w/landing-1/data/store.tsv", "/w/tree-030/x"])
    );
}

#[test]
fn a_prefix_that_is_not_where_a_path_starts_is_refused() {
    let mut data = encoded(&["src/lib.rs"]);
    data.extend_from_slice(b"junk /w/tree-03/x");
    assert_eq!(
        rewrite_dep_info(&data, b"/w/tree-03/", b"/w/tree-05/"),
        None
    );
}

#[test]
fn text_is_replaced_everywhere() {
    assert_eq!(
        replace(b"a=/w/t1/x b=/w/t1/y", b"/w/t1/", b"/w/tree-1/"),
        b"a=/w/tree-1/x b=/w/tree-1/y".to_vec()
    );
}
