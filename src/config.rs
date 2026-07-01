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
    /// Currently unread: `generate_api_file` always imports `typed_result_type` instead (see the
    /// `TypedApiResult` doc comment in the consuming project's `http.ts`-equivalent module for
    /// why), but `result_type` itself is still read by `type_extract` for peeling handler return
    /// types, so this stays alongside it rather than being removed outright.
    #[expect(
        dead_code,
        reason = "kept for parity with result_type; not currently read"
    )]
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
    /// `APIM_TYPED_RESULT_TYPE` — name of the discriminated response-wrapper type used when
    /// `field_errors` is set on an endpoint (default: `"TypedApiResult"`).
    pub(crate) typed_result_type: String,
    /// `APIM_TYPED_RESULT_PATH` — TS module path to import `typed_result_type` from
    /// (default: `"bindings/ApiResult"`).
    pub(crate) typed_result_path: String,
    /// `APIM_MODELS_PATH` — relative TS module path used as the parent
    /// directory when importing model types referenced by an endpoint's
    /// query/body/path-params/response roles (default: `".."`).
    pub(crate) models_path: String,
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
        let typed_result_type =
            std::env::var("APIM_TYPED_RESULT_TYPE").unwrap_or_else(|_| "TypedApiResult".to_owned());
        let typed_result_path = std::env::var("APIM_TYPED_RESULT_PATH")
            .unwrap_or_else(|_| "bindings/ApiResult".to_owned());
        let models_path = std::env::var("APIM_MODELS_PATH").unwrap_or_else(|_| "..".to_owned());

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
            typed_result_type,
            typed_result_path,
            models_path,
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
            typed_result_type: "TypedApiResult".to_owned(),
            typed_result_path: "bindings/ApiResult".to_owned(),
            models_path: "..".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    /// `apim_env_var_list.txt` is the single source of truth for which `APIM_*` vars exist;
    /// `build.rs` registers each with `cargo:rerun-if-env-changed` so Cargo rebuilds this
    /// crate (and, transitively, consumers) when one changes. This test guards against
    /// `from_env` reading a var that was never added to the list.
    #[test]
    fn from_env_only_reads_listed_apim_vars() {
        let list = include_str!("../apim_env_var_list.txt");
        let config_src = include_str!("config.rs");

        for var in list.lines() {
            assert!(
                config_src.contains(var),
                "`{var}` is listed in apim_env_var_list.txt but never read in config.rs"
            );
        }

        for var in config_src
            .split("std::env::var(\"")
            .skip(1)
            .filter_map(|s| s.split('"').next())
        {
            assert!(
                list.lines().any(|listed| listed == var),
                "`{var}` is read via std::env::var in config.rs but missing from \
                 apim_env_var_list.txt, so build.rs won't rerun-if-env-changed on it"
            );
        }
    }
}
