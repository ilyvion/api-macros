//! Procedural macro for exporting actix-web API endpoint contracts as TypeScript.
//!
//! # Usage
//!
//! ```rust,ignore
//! #[api_endpoint(method = "GET", path = "users/profile")]
//! pub(crate) async fn get_user_profile(
//!     pool: web::Data<DbPool>,
//!     request: HttpRequest,
//!     filter: web::Query<FilterQuery>,
//! ) -> Result<Json<ApiResult<ProfileResponse>>> {
//!     // …
//! }
//! ```
//!
//! The macro:
//! - Applies actix-web's own route attribute (`#[actix_web::get(...)]`, `#[actix_web::post(...)]`,
//!   etc.) to the annotated function so it becomes an `HttpServiceFactory` usable with
//!   `cfg.service(fn_name)`. The path passed to the actix attribute has `depth` leading segments
//!   stripped (default: 1), matching the scope nesting at the registration call site.
//! - Emits a `#[cfg(test)]` test module whose tests, when run via `cargo test export_endpoint`,
//!   write the following files to disk:
//!   - `<APIM_EXPORT_DIR>/<APIM_ENDPOINTS_PATH>/<Name>.ts` — typed spec object
//!   - `<APIM_EXPORT_DIR>/<APIM_API_PATH>/<Name>.ts` — thin `callEndpoint` wrapper
//!   - `<APIM_EXPORT_DIR>/<APIM_ENDPOINTS_PATH>/EndpointSpec.ts` — shared `EndpointSpec` type
//!
//! # Required environment variables
//!
//! Set these in your project's `.cargo/config.toml` under `[env]`:
//!
//! - `APIM_CALL_ENDPOINT_MODULE` (**required**) — TypeScript module path for the import of
//!   `callEndpoint`, e.g. `"client/services/http"`.
//! - `APIM_CALL_ENDPOINT_NAME` (optional, default `"callEndpoint"`) — name of the function
//!   exported by `APIM_CALL_ENDPOINT_MODULE`.
//! - `APIM_RESULT_TYPE` (optional, default `"ApiResult"`) — name of the response-wrapper type.
//! - `APIM_RESULT_PATH` (optional, default `"bindings/ApiResult"`) — TS module path to import
//!   `APIM_RESULT_TYPE` from.
//! - `APIM_EXPORT_DIR` (optional, default `"generated"`) — root directory for all generated
//!   output files, relative to `CARGO_MANIFEST_DIR`.
//! - `APIM_ENDPOINTS_PATH` (optional, default `"bindings/endpoints"`) — sub-path under
//!   `APIM_EXPORT_DIR` for endpoint binding files, and the TS module path prefix used when
//!   importing spec files in the generated API wrappers.
//! - `APIM_API_PATH` (optional, default `"api"`) — sub-path under `APIM_EXPORT_DIR` for
//!   generated API wrapper files.
//! - `APIM_DEPTH_DEFAULT` (optional, default `1`) — default value for the `depth` macro
//!   argument when it is not specified in the attribute.
//! - `APIM_JSON_API_RESULT_ALIAS` (optional, default `""`) — if non-empty, this type name is
//!   accepted in handler return types as an alias for `Json<{APIM_RESULT_TYPE}<T>>`.
//! - `APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS` (optional, default `""`) — if non-empty,
//!   accepted as an alias for `CustomizeResponder<Json<{APIM_RESULT_TYPE}<T>>>`.
//! - `APIM_UNWRAPPED_RESPONSE` (optional, default `"false"`) — when set to `1`, `true`, or
//!   `yes`, switches to unwrapped mode: handler return types omit the `APIM_RESULT_TYPE` layer
//!   (e.g. `Result<Json<T>>` instead of `Result<Json<ApiResult<T>>>`), and generated TypeScript
//!   wrappers return `Promise<T>` directly instead of `Promise<ApiResult<T>>`.
//!
//! The generated TypeScript file contains `const` values for the method and path, and
//! `type` aliases for the query, body, path-params, and response roles.

use heck::{ToLowerCamelCase as _, ToPascalCase as _};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, ItemFn, LitStr, parse_macro_input};

const ENDPOINT_SPEC_TS: &str = include_str!("../EndpointSpec.ts");

mod args;
mod config;
mod ts_gen;
mod type_extract;

use args::EndpointArgs;
use config::MacroConfig;
use type_extract::{ExtractedRole, extract_types, is_unit};

/// Attribute macro that exports an actix-web handler's endpoint contract to TypeScript.
///
/// # Arguments
/// - `method` — HTTP verb string literal, e.g. `"GET"` (required).
/// - `path`   — URL path relative to `/api/`, e.g. `"users/profile"` (required).
/// - `name`   — TypeScript name override (optional; defaults to the name derived from
///   `method` + `path`, e.g. `"GET"` + `"users/profile"` → `"GetUsersProfile"`).
/// - `depth`  — Number of leading path segments to strip when forming the actix route path
///   (optional; defaults to `APIM_DEPTH_DEFAULT`, which itself defaults to `1`). Because the macro emits `#[actix_web::get("…")]` (or the
///   appropriate verb), the path it passes to actix must be relative to the scope the handler
///   is registered under, not the full canonical path. `depth = 1` strips one segment (e.g.
///   `"user/info"` → `"info"` inside a `/user` scope); `depth = 2` strips two for doubly-nested
///   scopes. The full `path` argument is always used in the generated TypeScript path constant,
///   regardless of `depth`.
/// - `field_errors` — name of a Rust type describing the per-message field tag carried on
///   `success: false` responses (optional). When set, the generated TypeScript exports a
///   `{Name}FieldErrors` alias for it and the API wrapper's response type becomes
///   `TypedApiResult<{Name}Response, {Name}FieldErrors>` instead of `{Name}Response`'s plain
///   result wrapper.
///
/// # Generated items
/// 1. The handler function annotated with actix-web's route macro (e.g. `#[actix_web::get("…")]`),
///    turning it into an `HttpServiceFactory` usable via `cfg.service(fn_name)`.
/// 2. `<APIM_EXPORT_DIR>/bindings/endpoints/<Name>.ts` — written by a `#[test]` when
///    `cargo test export_endpoint` is run (matches the same naming convention as ts-rs's
///    `cargo test export_bindings`).
/// 3. `<APIM_EXPORT_DIR>/api/<Name>.ts` — a thin typed wrapper around `callEndpoint`, also
///    written by a `#[test]` when `cargo test export_endpoint` is run.
/// 4. `<APIM_EXPORT_DIR>/<APIM_ENDPOINTS_PATH>/EndpointSpec.ts` — the shared `EndpointSpec`
///    type definition, copied verbatim alongside the generated endpoint files.
#[proc_macro_attribute]
pub fn api_endpoint(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as EndpointArgs);
    let func = parse_macro_input!(item as ItemFn);

    match expand(&args, &func) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Derive a `PascalCase` TypeScript name from an HTTP method and path.
///
/// The method is capitalised as a word (`"GET"` → `"Get"`), and each path segment is
/// converted to `PascalCase` (`"users/profile"` → `"UsersProfile"`). Path
/// parameter placeholders have their braces stripped before conversion (`{id}` → `"Id"`).
///
/// Examples:
/// - `"GET"` + `"users/profile"` → `"GetUsersProfile"`
/// - `"DELETE"` + `"user/webauthn/credentials/{id}"` → `"DeleteUserWebauthnCredentialsId"`
/// - `"POST"` + `"auth/reset-password/complete"` → `"PostAuthResetPasswordComplete"`
/// Strip `depth` leading path segments to produce an actix-web route path.
///
/// A trailing slash is preserved: `"itemdata/"` with depth 1 yields `"/"`, not `""`.
fn route_path_after_depth(path: &str, depth: usize) -> String {
    let joined = path.split('/').skip(depth).collect::<Vec<_>>().join("/");
    if joined.is_empty() && path.ends_with('/') {
        "/".to_string()
    } else {
        joined
    }
}

fn ts_name_from_method_and_path(method: &str, path: &str) -> String {
    let method_part = method.to_pascal_case();
    let path_part: String = path
        .split('/')
        .map(|segment| {
            // Strip path parameter braces: `{id}` → `id`, `{userId}` → `userId`
            let seg = segment.trim_matches(|c: char| c == '{' || c == '}');
            seg.to_pascal_case()
        })
        .collect::<Vec<_>>()
        .concat();

    format!("{method_part}{path_part}")
}

#[expect(
    clippy::too_many_lines,
    reason = "proc-macro expansion is inherently verbose"
)]
fn expand(args: &EndpointArgs, func: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let method_str = args.method.value();
    let path_str = args.path.value();

    let config = MacroConfig::from_env();

    // Validate the HTTP method early and derive the actix-web route attribute name (e.g. `get`).
    // Doing this before type extraction lets us fail fast with a clear error.
    let method_lower = method_str.to_lowercase();
    let method_fn = match method_lower.as_str() {
        "get" | "post" | "put" | "patch" | "delete" | "head" | "options" => {
            Ident::new(&method_lower, Span::call_site())
        }
        _ => {
            return Err(syn::Error::new_spanned(
                &args.method,
                format!(
                    "#[api_endpoint]: unknown HTTP method `{method_str}`; \
                     expected GET, POST, PUT, PATCH, DELETE, HEAD, or OPTIONS"
                ),
            ));
        }
    };

    // Resolve the `depth` argument (defaults to `APIM_DEPTH_DEFAULT`, which itself defaults to 1).
    let depth: usize = args
        .depth
        .as_ref()
        .map(syn::LitInt::base10_parse::<usize>)
        .transpose()
        .map_err(|e| syn::Error::new(e.span(), format!("#[api_endpoint]: {e}")))?
        .unwrap_or(config.depth_default);

    let route_path = route_path_after_depth(&path_str, depth);

    // Validate: depth must leave at least one segment (or an empty string for the scope root).
    let segment_count = path_str.split('/').count();
    if depth > segment_count {
        // Point the span at `depth` if provided, otherwise at `path`.
        let err_span = args
            .depth
            .as_ref()
            .map_or_else(|| args.path.span(), syn::LitInt::span);
        return Err(syn::Error::new(
            err_span,
            format!(
                "#[api_endpoint]: `depth` ({depth}) exceeds the number of path segments \
                 ({segment_count}) in path `\"{path_str}\"`"
            ),
        ));
    }

    let route_path_lit = LitStr::new(&route_path, Span::call_site());

    // Derive the TypeScript name: either the explicit override or derived from method + path.
    let fn_name = &func.sig.ident;
    let ts_name = args.name.as_ref().map_or_else(
        || ts_name_from_method_and_path(&method_str, &path_str),
        syn::LitStr::value,
    );

    // camelCase TypeScript function name for the generated API wrapper, e.g.
    // "GetUserInfo" → "getUserInfo", "PostAuthWebauthn2faComplete" → "postAuthWebauthn2faComplete".
    let api_fn_name = ts_name.to_lower_camel_case();

    let extract_config = type_extract::ExtractConfig {
        result_type: &config.result_type,
        json_alias: config.json_alias.as_deref(),
        customized_alias: config.customized_alias.as_deref(),
        unwrapped_response: config.unwrapped_response,
    };

    // Extract types from the function signature.
    let extracted = extract_types(&func.sig, &extract_config)?;

    // Validate required env vars after type extraction so type errors surface first.
    let _ = config.call_endpoint_module()?;

    // Resolve optional types to concrete syn::Type values.
    let query_ty = extracted.query.as_deref();
    let body_ty = extracted.body.as_deref();
    let path_params_ty = extracted.path_params.as_deref();

    // Parse the optional `field_errors = "EnumName"` argument into a concrete type.
    let field_errors_ty = args
        .field_errors
        .as_ref()
        .map(|lit| {
            syn::parse_str::<syn::Type>(&lit.value()).map_err(|e| {
                syn::Error::new(
                    lit.span(),
                    format!("#[api_endpoint]: `field_errors` is not a valid type name: {e}"),
                )
            })
        })
        .transpose()?;

    // Validate: if the response is a union, both branches must differ (sanity check only).
    if let ExtractedRole::Union(a, b) = &extracted.response
        && is_unit(a)
        && is_unit(b)
    {
        return Err(syn::Error::new_spanned(
            fn_name,
            "#[api_endpoint]: both branches of Either resolve to `()` — \
             this is likely a mistake",
        ));
    }

    // Build the TypeScript file content at macro-expansion time.
    let ts_content = ts_gen::generate_ts_file(
        &ts_name,
        &method_str,
        &path_str,
        query_ty,
        body_ty,
        path_params_ty,
        &extracted.response,
        field_errors_ty.as_ref(),
    )?;

    // Build the API wrapper content (thin callEndpoint wrapper).
    let api_content = ts_gen::generate_api_file(
        &ts_name,
        &api_fn_name,
        ts_gen::ApiFileRoles {
            has_query: extracted.query.is_some(),
            has_body: extracted.body.is_some(),
            has_path_params: extracted.path_params.is_some(),
            has_field_errors: field_errors_ty.is_some(),
        },
        &config,
    );

    // Output file paths (relative to CARGO_MANIFEST_DIR, i.e. the workspace root).
    let ts_file = format!(
        "{}/{}/{ts_name}.ts",
        config.export_dir, config.endpoints_path
    );
    let api_file = format!("{}/{}/{ts_name}.ts", config.export_dir, config.api_path);
    let endpoint_spec_ts_file = format!(
        "{}/{}/EndpointSpec.ts",
        config.export_dir, config.endpoints_path
    );

    // Generate the test module name and test function names from the Rust fn name.
    let test_mod_name = Ident::new(&format!("__export_endpoint_{fn_name}"), Span::call_site());
    let test_fn_name = Ident::new(&format!("export_endpoint_{fn_name}"), Span::call_site());
    let test_api_fn_name = Ident::new(&format!("export_endpoint_api_{fn_name}"), Span::call_site());
    let test_endpoint_spec_ts_fn_name = Ident::new(
        &format!("export_endpoint_spec_ts_{fn_name}"),
        Span::call_site(),
    );

    let expanded = quote! {
        // Apply actix-web's route macro to the function, making it an HttpServiceFactory
        // that can be registered with `cfg.service(#fn_name)`.
        #[::actix_web::#method_fn(#route_path_lit)]
        #func

        #[cfg(test)]
        #[allow(non_snake_case)]
        mod #test_mod_name {
            #[test]
            fn #test_fn_name() {
                let content: &str = #ts_content;
                let out = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(#ts_file);
                ::std::fs::create_dir_all(out.parent().expect("parent dir exists"))
                    .expect("create bindings/endpoints dir");
                ::std::fs::write(&out, content)
                    .expect("write endpoint TypeScript binding");
            }

            #[test]
            fn #test_api_fn_name() {
                let content: &str = #api_content;
                let out = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(#api_file);
                ::std::fs::create_dir_all(out.parent().expect("parent dir exists"))
                    .expect("create api dir");
                ::std::fs::write(&out, content)
                    .expect("write API wrapper TypeScript file");
            }

            #[test]
            fn #test_endpoint_spec_ts_fn_name() {
                let content: &str = #ENDPOINT_SPEC_TS;
                let out = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(#endpoint_spec_ts_file);
                ::std::fs::create_dir_all(out.parent().expect("parent dir exists"))
                    .expect("create endpoints dir");
                ::std::fs::write(&out, content)
                    .expect("write EndpointSpec.ts type definitions");
            }
        }
    };

    Ok(expanded)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_name_multi_segment() {
        assert_eq!(
            ts_name_from_method_and_path("GET", "users/profile"),
            "GetUsersProfile"
        );
    }

    #[test]
    fn ts_name_single_segment() {
        assert_eq!(
            ts_name_from_method_and_path("DELETE", "users"),
            "DeleteUsers"
        );
    }

    #[test]
    fn ts_name_path_param_braces_stripped() {
        assert_eq!(
            ts_name_from_method_and_path("DELETE", "user/webauthn/credentials/{id}"),
            "DeleteUserWebauthnCredentialsId"
        );
    }

    #[test]
    fn ts_name_hyphenated_segments() {
        assert_eq!(
            ts_name_from_method_and_path("POST", "auth/reset-password/complete"),
            "PostAuthResetPasswordComplete"
        );
    }

    #[test]
    fn ts_name_camel_case_segment() {
        // Existing camelCase segment is correctly PascalCased.
        assert_eq!(
            ts_name_from_method_and_path("GET", "users/recentItems"),
            "GetUsersRecentItems"
        );
    }

    #[test]
    fn route_path_trailing_slash_becomes_slash() {
        // "itemdata/" with depth 1 must yield "/" not "".
        assert_eq!(route_path_after_depth("itemdata/", 1), "/");
    }

    #[test]
    fn route_path_multi_segment_trailing_slash_preserved() {
        assert_eq!(route_path_after_depth("api/items/", 1), "items/");
    }

    #[test]
    fn route_path_no_trailing_slash() {
        assert_eq!(route_path_after_depth("api/items", 1), "items");
    }

    #[test]
    fn route_path_single_segment_no_trailing_slash_yields_empty() {
        // "itemdata" with depth 1 → "" (scope root without trailing slash).
        assert_eq!(route_path_after_depth("itemdata", 1), "");
    }

    #[test]
    fn route_path_depth_zero() {
        assert_eq!(route_path_after_depth("items/", 0), "items/");
    }

    #[test]
    fn ts_name_method_lowercased_to_title() {
        // All-caps HTTP verbs become title-case words.
        assert_eq!(
            ts_name_from_method_and_path("PATCH", "user/info"),
            "PatchUserInfo"
        );
        assert_eq!(
            ts_name_from_method_and_path("PUT", "user/customViews"),
            "PutUserCustomViews"
        );
    }
}
