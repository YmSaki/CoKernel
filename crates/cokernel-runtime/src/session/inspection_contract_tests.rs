mod inspection_contract_tests {
    use super::*;

    fn supported_value(name: &str, value: Value) -> Value {
        json!({
            "name": name,
            "type_module": "builtins",
            "type_name": "int",
            "supported": true,
            "value": value,
            "reason": null,
        })
    }

    #[test]
    fn inspection_list_contract_accepts_unique_bounded_metadata() {
        let variables = validate_variable_list_result(json!({
            "variables": [
                {
                    "name": "x",
                    "type_module": "builtins",
                    "type_name": "int",
                    "supported": true
                },
                {
                    "name": "secret",
                    "type_module": "user",
                    "type_name": "Secret",
                    "supported": false
                }
            ]
        }))
        .unwrap();
        assert_eq!(variables.len(), 2);
    }

    #[test]
    fn inspection_list_contract_rejects_duplicate_names() {
        let result = validate_variable_list_result(json!({
            "variables": [
                {"name": "x", "type_module": "builtins", "type_name": "int", "supported": true},
                {"name": "x", "type_module": "builtins", "type_name": "str", "supported": true}
            ]
        }));
        assert!(matches!(result, Err(SessionInspectionError::InvalidResponse(_))));
    }

    #[test]
    fn inspection_list_contract_rejects_too_many_entries() {
        let variables = (0..=MAX_INSPECTION_LIST_ITEMS)
            .map(|index| {
                json!({
                    "name": format!("value_{index}"),
                    "type_module": "builtins",
                    "type_name": "int",
                    "supported": true
                })
            })
            .collect::<Vec<_>>();
        assert!(validate_variable_list_result(json!({"variables": variables})).is_err());
    }

    #[test]
    fn inspection_get_contract_distinguishes_supported_none_from_missing_value() {
        let none_value =
            validate_variable_value_result(supported_value("value", Value::Null), "value")
                .unwrap();
        assert!(none_value.supported);
        assert_eq!(none_value.value, None);
        assert_eq!(none_value.reason, None);

        let missing = validate_variable_value_result(
            json!({
                "name": "value",
                "type_module": "builtins",
                "type_name": "NoneType",
                "supported": true,
                "reason": null
            }),
            "value",
        );
        assert!(missing.is_err());
    }

    #[test]
    fn inspection_get_contract_rejects_supported_reason_and_wrong_name() {
        let with_reason = validate_variable_value_result(
            json!({
                "name": "value",
                "type_module": "builtins",
                "type_name": "int",
                "supported": true,
                "value": 1,
                "reason": "unexpected"
            }),
            "value",
        );
        assert!(with_reason.is_err());

        assert!(
            validate_variable_value_result(supported_value("other", json!(1)), "value").is_err()
        );
    }

    #[test]
    fn inspection_get_contract_rejects_malformed_unsupported_value() {
        let non_null = validate_variable_value_result(
            json!({
                "name": "value",
                "type_module": "user",
                "type_name": "Secret",
                "supported": false,
                "value": {"leak": true},
                "reason": "unsupported variable type: user.Secret"
            }),
            "value",
        );
        assert!(non_null.is_err());

        let missing_reason = validate_variable_value_result(
            json!({
                "name": "value",
                "type_module": "user",
                "type_name": "Secret",
                "supported": false,
                "value": null,
                "reason": null
            }),
            "value",
        );
        assert!(missing_reason.is_err());
    }

    #[test]
    fn inspection_contract_rejects_empty_and_oversized_metadata() {
        let empty_name = validate_variable_list_result(json!({
            "variables": [{
                "name": "",
                "type_module": "builtins",
                "type_name": "int",
                "supported": true
            }]
        }));
        assert!(empty_name.is_err());

        let huge = "x".repeat(MAX_INSPECTION_METADATA_CHARS + 1);
        assert!(validate_variable_value_result(
            json!({
                "name": "value",
                "type_module": huge,
                "type_name": "Secret",
                "supported": false,
                "value": null,
                "reason": "unsupported"
            }),
            "value",
        )
        .is_err());
    }

    #[test]
    fn inspection_contract_rejects_oversized_serialized_result() {
        let huge = "x".repeat(MAX_INSPECTION_RESPONSE_BYTES);
        assert!(validate_variable_value_result(
            json!({
                "name": "value",
                "type_module": "builtins",
                "type_name": "str",
                "supported": true,
                "value": huge,
                "reason": null
            }),
            "value",
        )
        .is_err());
    }
}
