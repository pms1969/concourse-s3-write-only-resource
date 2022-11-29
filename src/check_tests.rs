use crate::OutParams;
use crate::S3WriteOnly;
use crate::Source;
use crate::Version;
use concourse_resource::Resource;

#[cfg(test)]
#[test]
fn check_no_previous() {
    //* Specifically test the case were we are running for the first time and do not
    //* have any previous version.

    // Set up the source.
    let source = Source {
        ..Default::default()
    };

    let version = S3WriteOnly::resource_check(Some(source), None);

    // And in theory, we should get back an empty vector of versions.
    assert_eq!(version.len(), 0);
}

#[test]
fn check_with_new_and_no_previous() {
    //* Specifically test the case were we are running have been passed a version, which would have come from the last get.

    // Set up the source.
    let source = Source {
        ..Default::default()
    };

    let version = S3WriteOnly::resource_check(
        Some(source),
        Some(Version {
            path: String::from("test/file.txt"),
        }),
    );

    // And in theory, we should get back 1 version, which points at the path we sent files to.
    assert_eq!(version.len(), 1);
    assert_eq!(version[0].path, "test/file.txt");
}

#[test]
fn test_serialisation() {
    let s =
        r#"{ "glob": "output/**/*.txt", "except_regex": "test.txt", "s3_prefix": "/test/thing" }"#;
    let op: OutParams = serde_json::from_str(s).unwrap();
    assert_eq!(&op.glob, "output/**/*.txt");
    assert!(op.except_regex.is_some());
    assert_eq!(op.except_regex.clone().unwrap().as_str(), "test.txt");
    assert!(op
        .except_regex
        .clone()
        .unwrap()
        .is_match("/output/testdir1/test.txt"));
    assert!(!op
        .except_regex
        .clone()
        .unwrap()
        .is_match("/output/testdir1/test1.txt"));
}
