//! `Recipe.validation` decodes into [`RecipeValidation`] and encodes back to
//! the Control Plane's JSON, nulls dropped. A status or severity this SDK has
//! not seen reads as `Other`.

use introspection_sdk::{
    Recipe, RecipeValidation, RecipeValidationDiagnostic, RecipeValidationSeverity,
    RecipeValidationSpan, RecipeValidationStatus,
};
use serde_json::{json, Value};

fn recipe(validation: Value) -> Value {
    json!({
        "id": "11111111-1111-7111-8111-111111111111",
        "org_id": "22222222-2222-7222-8222-222222222222",
        "project_id": "33333333-3333-7333-8333-333333333333",
        "repository_id": "44444444-4444-7444-8444-444444444444",
        "name": "hello-mcp",
        "slug": "hello-mcp-main-543b8db50a83",
        "git_ref": "main",
        "git_commit_sha": "543b8db50a83543b8db50a83543b8db50a83543b",
        "validation": validation,
        "created_by_member_id": "55555555-5555-7555-8555-555555555555",
        "created_at": "2026-10-28T10:18:40Z",
        "updated_at": "2026-10-28T10:18:42Z"
    })
}

#[test]
fn an_invalid_validation_decodes_its_diagnostics_and_round_trips() {
    let validation = json!({
        "status": "invalid",
        "validator_name": "introspection-recipe-check",
        "validator_version": "0.12.0",
        "checked_at": "2026-10-28T10:18:42.310504Z",
        "diagnostics": [
            {
                "code": "policy.parse",
                "severity": "error",
                "path": "policies/main.cedar",
                "span": {"line": 3, "column": 7},
                "message": "unexpected token `permit`",
                "help": "end the previous statement with `;`"
            },
            {
                "code": "mcp.unused",
                "severity": "warning",
                "path": "package.json",
                "message": "MCP server `echo` is declared but never called"
            }
        ]
    });
    let body = recipe(validation.clone());

    let recipe: Recipe = serde_json::from_value(body.clone()).unwrap();
    let decoded = recipe.validation.as_ref().unwrap();
    assert_eq!(
        decoded,
        &RecipeValidation {
            status: RecipeValidationStatus::Invalid,
            validator_name: Some("introspection-recipe-check".into()),
            validator_version: Some("0.12.0".into()),
            checked_at: Some("2026-10-28T10:18:42.310504Z".into()),
            error_message: None,
            diagnostics: vec![
                RecipeValidationDiagnostic {
                    code: "policy.parse".into(),
                    severity: RecipeValidationSeverity::Error,
                    path: "policies/main.cedar".into(),
                    span: Some(RecipeValidationSpan { line: 3, column: 7 }),
                    message: "unexpected token `permit`".into(),
                    help: Some("end the previous statement with `;`".into()),
                },
                RecipeValidationDiagnostic {
                    code: "mcp.unused".into(),
                    severity: RecipeValidationSeverity::Warning,
                    path: "package.json".into(),
                    span: None,
                    message: "MCP server `echo` is declared but never called".into(),
                    help: None,
                },
            ],
        }
    );

    assert_eq!(serde_json::to_value(&recipe).unwrap(), body);
}

#[test]
fn a_pending_validation_drops_its_nulls_and_keeps_empty_diagnostics() {
    let recipe: Recipe = serde_json::from_value(recipe(json!({
        "status": "pending",
        "validator_name": null,
        "validator_version": null,
        "checked_at": null,
        "error_message": null,
        "diagnostics": []
    })))
    .unwrap();
    assert_eq!(
        serde_json::to_value(recipe.validation.unwrap()).unwrap(),
        json!({"status": "pending", "diagnostics": []})
    );
}

#[test]
fn a_new_status_and_severity_read_as_other_and_encode_back() {
    let validation = json!({
        "status": "superseded",
        "diagnostics": [
            {"code": "x", "severity": "info", "path": "a", "message": "m"}
        ]
    });
    let decoded: RecipeValidation = serde_json::from_value(validation.clone()).unwrap();
    assert_eq!(
        decoded.status,
        RecipeValidationStatus::Other("superseded".into())
    );
    assert_eq!(
        decoded.diagnostics[0].severity,
        RecipeValidationSeverity::Other("info".into())
    );
    assert_eq!(serde_json::to_value(&decoded).unwrap(), validation);
}
