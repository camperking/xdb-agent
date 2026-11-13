pub mod file_reader;
pub mod js_repl;
pub mod time;
pub mod weather;
pub mod web_fetch;
pub mod web_search;

pub use file_reader::ReadFile;
pub use js_repl::JsRepl;
pub use time::GetCurrentTime;
pub use weather::GetWeather;
pub use web_fetch::WebFetch;
pub use web_search::WebSearch;

#[cfg(test)]
mod tests {
    use super::file_reader::ReadFileArgs;
    use super::js_repl::JsReplArgs;
    use super::time::GetCurrentTimeArgs;
    use super::*;
    use rig::completion::ToolDefinition;
    use rig::tool::{Tool, ToolContext};

    fn definition<T: Tool>(tool: &T) -> ToolDefinition {
        rig::tool::tool_definition(tool)
    }

    #[test]
    fn every_tool_exposes_its_name_description_and_schema() {
        let definitions = [
            definition(&GetCurrentTime),
            definition(&GetWeather),
            definition(&ReadFile),
            definition(&WebFetch),
            definition(&WebSearch),
            definition(&JsRepl),
        ];

        let names: Vec<&str> = definitions.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "get_time",
                "get_weather",
                "read_file",
                "web_fetch",
                "web_search",
                "js_repl"
            ]
        );

        for d in &definitions {
            assert!(!d.description.is_empty(), "{} has no description", d.name);
            assert!(
                d.parameters.is_object(),
                "{} schema is not an object",
                d.name
            );
        }
    }

    #[test]
    fn tool_schemas_keep_their_required_arguments() {
        for (d, required) in [
            (definition(&GetWeather), "location"),
            (definition(&ReadFile), "path"),
            (definition(&WebFetch), "url"),
            (definition(&WebSearch), "query"),
            (definition(&JsRepl), "code"),
        ] {
            assert!(
                d.parameters["properties"].get(required).is_some(),
                "{} schema lost the `{required}` property",
                d.name
            );
            assert!(
                d.parameters["required"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == required)),
                "{} schema no longer requires `{required}`",
                d.name
            );
        }
    }

    #[tokio::test]
    async fn js_repl_returns_the_evaluated_value() {
        let mut context = ToolContext::new();
        let output = JsRepl
            .call(&mut context, JsReplArgs { code: "2 * 21".to_string() })
            .await
            .unwrap();

        assert_eq!(output, "42");
    }

    #[tokio::test]
    async fn js_repl_rejects_empty_code() {
        let mut context = ToolContext::new();
        let error = JsRepl
            .call(&mut context, JsReplArgs { code: "   ".to_string() })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("code must not be empty"));
    }

    #[tokio::test]
    async fn read_file_reports_the_path_it_could_not_read() {
        let mut context = ToolContext::new();
        let error = ReadFile
            .call(
                &mut context,
                ReadFileArgs {
                    path: "/nonexistent/xdb-agent-test".to_string(),
                },
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("/nonexistent/xdb-agent-test"));
    }

    #[tokio::test]
    async fn get_time_formats_the_current_utc_timestamp() {
        let mut context = ToolContext::new();
        let output = GetCurrentTime
            .call(&mut context, GetCurrentTimeArgs {})
            .await
            .unwrap();

        assert!(output.starts_with("Current time: "));
        assert!(output.ends_with("UTC"));
    }
}
