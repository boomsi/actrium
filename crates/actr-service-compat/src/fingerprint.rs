//! Semantic fingerprint calculation for protocol services
//!
//! Provides semantic fingerprinting capabilities for proto files and services
//! using proto-sign analysis that ignores formatting differences.

use crate::spec_builder::{ACTR_OPTIONS_IMPORT, self_contained_proto_hash};
use crate::{CompatibilityError, ProtoFile, Result};
use sha2::{Digest, Sha256};

/// Semantic fingerprint calculation utilities for protocol services
pub struct Fingerprint;

impl Fingerprint {
    // =========================================================================
    // Level 1: Single Proto File Fingerprinting
    // =========================================================================

    /// Calculate semantic fingerprint for a single proto file
    /// This uses proto-sign analysis and ignores formatting differences
    pub fn calculate_proto_semantic_fingerprint(proto_content: &str) -> Result<String> {
        let hash =
            semantic_proto_hash(proto_content).map_err(CompatibilityError::ProtoSignError)?;
        Ok(format!("semantic:{hash}"))
    }

    // Removed: calculate_proto_fingerprints() - content fingerprints are not semantically meaningful

    // =========================================================================
    // Level 2: Service-Level Multi-File Fingerprinting
    // =========================================================================

    /// Calculate semantic fingerprint for multiple proto files (service level)
    /// Uses proto-sign for each file and combines the results deterministically
    pub fn calculate_service_semantic_fingerprint(proto_files: &[ProtoFile]) -> Result<String> {
        let mut semantic_fingerprints = Vec::with_capacity(proto_files.len());
        for file in proto_files {
            let hash = semantic_proto_hash(&file.content).map_err(|e| {
                CompatibilityError::ProtoParseError {
                    file_name: file.name.clone(),
                    source: e,
                }
            })?;
            semantic_fingerprints.push((file.name.clone(), hash));
        }
        Ok(Self::service_fingerprint_from_hashes(semantic_fingerprints))
    }

    pub(crate) fn service_fingerprint_from_hashes(mut hashes: Vec<(String, String)>) -> String {
        hashes.sort_by(|a, b| a.0.cmp(&b.0));
        let combined = hashes
            .into_iter()
            .map(|(name, hash)| format!("{name}:{hash}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("service_semantic:{:x}", Sha256::digest(combined.as_bytes()))
    }

    // Removed: calculate_service_fingerprints() - content fingerprints are not semantically meaningful
}

fn semantic_proto_hash(source: &str) -> anyhow::Result<String> {
    let dependencies = protobuf_parse::pure::parse_dependencies(source)?;
    if dependencies
        .dependency
        .iter()
        .any(|path| path == ACTR_OPTIONS_IMPORT)
    {
        return self_contained_proto_hash(source);
    }
    Ok(proto_fingerprint::Spec::try_from(source)?.fingerprint)
}

// =========================================================================
// Fingerprint Result Structures
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use actr_protocol::ServiceSpec;

    fn create_test_proto_file(name: &str, content: &str) -> ProtoFile {
        ProtoFile {
            name: name.to_string(),
            content: content.to_string(),
            path: None,
        }
    }

    fn create_test_service(name: &str, proto_files: Vec<ProtoFile>) -> ServiceSpec {
        let semantic_fp = Fingerprint::calculate_service_semantic_fingerprint(&proto_files)
            .unwrap_or_else(|_| "placeholder".to_string());

        ServiceSpec {
            name: name.to_string(),
            description: Some("Test service".to_string()),
            fingerprint: semantic_fp,
            protobufs: proto_files
                .into_iter()
                .map(|pf| actr_protocol::service_spec::Protobuf {
                    package: pf.name, // Use name as package
                    content: pf.content,
                    fingerprint: "test-fp".to_string(),
                })
                .collect(),
            published_at: None,
            tags: vec![],
        }
    }

    #[test]
    fn test_semantic_fingerprint_ignores_formatting() {
        let content1 = "syntax=\"proto3\";message Test{string name=1;}";
        let content2 = r#"
            syntax = "proto3";

            // Test message
            message Test {
                string name = 1;  // Name field
            }
        "#;

        // Semantic fingerprints should be the same (semantically identical)
        let semantic_fp1 = Fingerprint::calculate_proto_semantic_fingerprint(content1).unwrap();
        let semantic_fp2 = Fingerprint::calculate_proto_semantic_fingerprint(content2).unwrap();

        assert_eq!(semantic_fp1, semantic_fp2);
    }

    #[test]
    fn actr_options_resolve_for_fingerprints_and_packed_contracts() {
        let source = r#"
            syntax = "proto3";
            package test;
            import "actr/options.proto";
            message Request {}
            service Test {
                rpc Run(Request) returns (Request) {
                    option (actr.reply_mode) = REPLY_MODE_EXPLICIT;
                }
            }
        "#;
        let legacy = Fingerprint::calculate_proto_semantic_fingerprint(source).unwrap();
        let packaged = crate::build_self_contained_service_spec(
            "test",
            ProtoFile {
                name: "test.proto".into(),
                content: source.into(),
                path: None,
            },
        )
        .unwrap();
        assert_eq!(legacy, packaged.protobufs[0].fingerprint);
        assert!(
            crate::spec_builder::self_contained_unary_routes(source)
                .unwrap()
                .contains("test.Test.Run")
        );
    }

    #[test]
    fn test_service_semantic_fingerprint() {
        let files = vec![
            create_test_proto_file(
                "a.proto",
                "syntax = \"proto2\"; message A { required string name = 1; }",
            ),
            create_test_proto_file(
                "b.proto",
                "syntax = \"proto2\"; message B { required int32 id = 1; }",
            ),
        ];

        let fp = Fingerprint::calculate_service_semantic_fingerprint(&files).unwrap();
        assert!(fp.starts_with("service_semantic:"));
    }

    #[test]
    fn test_service_spec_has_semantic_fingerprint() {
        let proto_file = create_test_proto_file(
            "test.proto",
            r#"
            syntax = "proto3";
            message TestMessage {
                string name = 1;
            }
        "#,
        );

        let service = create_test_service("test-service", vec![proto_file]);

        // ServiceSpec should have fingerprint field
        assert!(!service.fingerprint.is_empty());
    }

    #[test]
    fn test_proto_semantic_fingerprint_error_handling() {
        // Test with invalid proto content
        let invalid_proto = "this is not a valid proto file";
        let result = Fingerprint::calculate_proto_semantic_fingerprint(invalid_proto);
        assert!(result.is_err());

        if let Err(e) = result {
            assert!(matches!(e, CompatibilityError::ProtoSignError(_)));
        }
    }

    #[test]
    fn test_service_semantic_fingerprint_error_handling() {
        // Test with invalid proto content in service
        let invalid_file = create_test_proto_file("invalid.proto", "invalid proto content");
        let result = Fingerprint::calculate_service_semantic_fingerprint(&[invalid_file]);

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, CompatibilityError::ProtoParseError { .. }));
        }
    }

    #[test]
    fn test_service_semantic_fingerprint_deterministic() {
        // Test that file order doesn't affect fingerprint (files are sorted internally)
        let files_order1 = vec![
            create_test_proto_file(
                "z.proto",
                "syntax = \"proto3\"; message Z { string id = 1; }",
            ),
            create_test_proto_file(
                "a.proto",
                "syntax = \"proto3\"; message A { string name = 1; }",
            ),
        ];

        let files_order2 = vec![
            create_test_proto_file(
                "a.proto",
                "syntax = \"proto3\"; message A { string name = 1; }",
            ),
            create_test_proto_file(
                "z.proto",
                "syntax = \"proto3\"; message Z { string id = 1; }",
            ),
        ];

        let fp1 = Fingerprint::calculate_service_semantic_fingerprint(&files_order1).unwrap();
        let fp2 = Fingerprint::calculate_service_semantic_fingerprint(&files_order2).unwrap();

        assert_eq!(fp1, fp2, "File order should not affect fingerprint");
    }
}
