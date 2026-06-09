//! Environment-variable configuration for the `api_endpoint` macro.

use proc_macro2::Span;

/// All configuration values read from environment variables at macro-expansion time.
pub(crate) struct MacroConfig {
    /// `APIM_RESULT_TYPE` — name of the response-wrapper type (default: `"ApiResult"`).
    pub(crate) result_type: String,
    /// `APIM_JSON_API_RESULT_ALIAS` — accepted as an alias for `Json<result_type<T>>`.
    pub(crate) json_alias: Option<String>,
    /// `APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS` — accepted as an alias for
    /// `CustomizeResponder<Json<result_type<T>>>`.
    pub(crate) customized_alias: Option<String>,
    /// `APIM_CALL_ENDPOINT_MODULE` — TS module path for the `callEndpoint` import (required).
    /// `None` means the env var was not set; access via [`MacroConfig::call_endpoint_module`].
    call_endpoint_module: Option<String>,
    /// `APIM_CALL_ENDPOINT_NAME` — name of the `callEndpoint` function (default: `"callEndpoint"`).
    pub(crate) call_endpoint_name: String,
    /// `APIM_RESULT_PATH` — TS module path to import `result_type` from (default: `"bindings/ApiResult"`).
    pub(crate) result_path: String,
    /// `APIM_EXPORT_DIR` — root output directory relative to `CARGO_MANIFEST_DIR` (default: `"generated"`).
    pub(crate) export_dir: String,
    /// `APIM_ENDPOINTS_PATH` — sub-path under `export_dir` for endpoint binding files (default: `"bindings/endpoints"`).
    pub(crate) endpoints_path: String,
    /// `APIM_API_PATH` — sub-path under `export_dir` for API wrapper files (default: `"api"`).
    pub(crate) api_path: String,
    /// `APIM_DEPTH_DEFAULT` — default value for the `depth` macro argument (default: `1`).
    pub(crate) depth_default: usize,
    /// `APIM_UNWRAPPED_RESPONSE` — when `true`, handlers return `Result<Json<T>>` (no wrapper)
    /// and the generated TypeScript returns `Promise<T>` directly (default: `false`).
    pub(crate) unwrapped_response: bool,
}

impl MacroConfig {
    /// Read all `APIM_*` environment variables and return a populated [`MacroConfig`].
    ///
    /// Always succeeds; missing required variables are reported on first access via
    /// [`MacroConfig::call_endpoint_module`].
    pub(crate) fn from_env() -> Self {
        let result_type =
            std::env::var("APIM_RESULT_TYPE").unwrap_or_else(|_| "ApiResult".to_owned());

        let json_alias_str = std::env::var("APIM_JSON_API_RESULT_ALIAS").unwrap_or_default();
        let customized_alias_str =
            std::env::var("APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS").unwrap_or_default();

        let call_endpoint_module = std::env::var("APIM_CALL_ENDPOINT_MODULE").ok();
        let call_endpoint_name =
            std::env::var("APIM_CALL_ENDPOINT_NAME").unwrap_or_else(|_| "callEndpoint".to_owned());
        let result_path =
            std::env::var("APIM_RESULT_PATH").unwrap_or_else(|_| "bindings/ApiResult".to_owned());
        let export_dir =
            std::env::var("APIM_EXPORT_DIR").unwrap_or_else(|_| "generated".to_owned());
        let endpoints_path = std::env::var("APIM_ENDPOINTS_PATH")
            .unwrap_or_else(|_| "bindings/endpoints".to_owned());
        let api_path = std::env::var("APIM_API_PATH").unwrap_or_else(|_| "api".to_owned());
        let depth_default = std::env::var("APIM_DEPTH_DEFAULT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1);
        let unwrapped_response = std::env::var("APIM_UNWRAPPED_RESPONSE")
            .is_ok_and(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"));

        Self {
            result_type,
            json_alias: if json_alias_str.is_empty() {
                None
            } else {
                Some(json_alias_str)
            },
            customized_alias: if customized_alias_str.is_empty() {
                None
            } else {
                Some(customized_alias_str)
            },
            call_endpoint_module,
            call_endpoint_name,
            result_path,
            export_dir,
            endpoints_path,
            api_path,
            depth_default,
            unwrapped_response,
        }
    }

    /// Return the `APIM_CALL_ENDPOINT_MODULE` value, or a [`syn::Error`] if it was not set.
    pub(crate) fn call_endpoint_module(&self) -> syn::Result<&str> {
        self.call_endpoint_module.as_deref().ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "api_endpoint requires the `APIM_CALL_ENDPOINT_MODULE` environment variable \
                 to be set.\nAdd it to your project's `.cargo/config.toml`:\n\n\
                 [env]\nAPIM_CALL_ENDPOINT_MODULE = \"path/to/your/http/module\"\n\n\
                 The module must export a `callEndpoint`-compatible function. \
                 See the api-macros README for the required signature.",
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn for_tests(call_endpoint_module: impl Into<String>) -> Self {
        Self {
            result_type: "ApiResult".to_owned(),
            json_alias: None,
            customized_alias: None,
            call_endpoint_module: Some(call_endpoint_module.into()),
            call_endpoint_name: "callEndpoint".to_owned(),
            result_path: "bindings/ApiResult".to_owned(),
            export_dir: "generated".to_owned(),
            endpoints_path: "bindings/endpoints".to_owned(),
            api_path: "api".to_owned(),
            depth_default: 1,
            unwrapped_response: false,
        }
    }
}
