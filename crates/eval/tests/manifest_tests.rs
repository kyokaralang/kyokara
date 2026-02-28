#![allow(clippy::unwrap_used)]

use kyokara_eval::manifest::CapabilityManifest;

#[test]
fn parse_valid_manifest() {
    let json = r#"{"caps": {"io": {}, "Net": {}}}"#;
    let manifest = CapabilityManifest::from_json(json).unwrap();
    assert!(manifest.is_granted("io"));
    assert!(manifest.is_granted("Net"));
    assert!(!manifest.is_granted("Db"));
}

#[test]
fn parse_manifest_with_fine_grained_constraints() {
    let json = r#"{
        "caps": {
            "Net": { "allow_domains": ["example.com", "api.test.io"] },
            "Db":  { "allow_tables": ["users", "posts"], "allow_keys": ["id", "name"] }
        }
    }"#;
    let manifest = CapabilityManifest::from_json(json).unwrap();
    assert!(manifest.is_granted("Net"));
    assert!(manifest.is_granted("Db"));

    let net_grant = &manifest.caps["Net"];
    assert_eq!(
        net_grant.allow_domains.as_ref().unwrap(),
        &["example.com", "api.test.io"]
    );
    assert!(net_grant.allow_tables.is_none());

    let db_grant = &manifest.caps["Db"];
    assert_eq!(db_grant.allow_tables.as_ref().unwrap(), &["users", "posts"]);
    assert_eq!(db_grant.allow_keys.as_ref().unwrap(), &["id", "name"]);
}

#[test]
fn parse_empty_caps_manifest() {
    let json = r#"{"caps": {}}"#;
    let manifest = CapabilityManifest::from_json(json).unwrap();
    assert!(!manifest.is_granted("io"));
    assert!(!manifest.is_granted("Net"));
    assert!(manifest.caps.is_empty());
}

#[test]
fn parse_manifest_empty_grant() {
    let json = r#"{"caps": {"Clock": {}}}"#;
    let manifest = CapabilityManifest::from_json(json).unwrap();
    assert!(manifest.is_granted("Clock"));
    let grant = &manifest.caps["Clock"];
    assert!(grant.allow_domains.is_none());
    assert!(grant.allow_tables.is_none());
    assert!(grant.allow_keys.is_none());
}

#[test]
fn parse_manifest_from_string_invalid_json() {
    let result = CapabilityManifest::from_json("not json at all");
    assert!(result.is_err());
}

#[test]
fn parse_manifest_missing_caps_key() {
    // Missing "caps" key defaults to empty HashMap via serde(default).
    let json = r#"{}"#;
    let manifest = CapabilityManifest::from_json(json).unwrap();
    assert!(manifest.caps.is_empty());
    assert!(!manifest.is_granted("io"));
}

#[test]
fn parse_manifest_rejects_unknown_top_level_field() {
    let json = r#"{"caps": {"io": {}}, "unknown_field": true}"#;
    let result = CapabilityManifest::from_json(json);
    assert!(result.is_err(), "should reject unknown top-level field");
}

#[test]
fn parse_manifest_rejects_unknown_grant_field() {
    // Typo: allow_domain instead of allow_domains
    let json = r#"{"caps": {"io": {"allow_domain": ["example.com"]}}}"#;
    let result = CapabilityManifest::from_json(json);
    assert!(result.is_err(), "should reject unknown grant field (typo)");
}
