//! Procedural macro for exporting actix-web API endpoint contracts as TypeScript.
//!
//! # Usage
//!
//! ```rust,ignore
//! #[api_endpoint(method = "GET", path = "measurement/trends")]
//! pub(crate) async fn get_trends(
//!     pool: web::Data<DbPool>,
//!     request: HttpRequest,
//!     from_to_query: web::Query<FromToQuery>,
//! ) -> Result<JsonApiResult<TrendsResponse>> {
//!     // …
//! }
//! ```
//!
//! The macro:
//! - Applies actix-web's own route attribute (`#[actix_web::get(...)]`, `#[actix_web::post(...)]`,
//!   etc.) to the annotated function so it becomes an `HttpServiceFactory` usable with
//!   `cfg.service(fn_name)`. The path passed to the actix attribute has `depth` leading segments
//!   stripped (default: 1), matching the scope nesting at the registration call site.
//! - Emits a `#[cfg(test)]` test module whose single test writes
//!   `bindings/endpoints/<Name>.ts` to disk when `cargo test export_endpoint` is run.
//!
//! The generated TypeScript file contains `const` values for the method and path, and
//! `type` aliases for the query, body, path-params, and response roles.

use heck::{ToLowerCamelCase as _, ToPascalCase as _};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, ItemFn, LitStr, parse_macro_input};

mod args;
mod ts_gen;
mod type_extract;

use args::EndpointArgs;
use type_extract::{ExtractedRole, extract_types, is_unit};

/// Attribute macro that exports an actix-web handler's endpoint contract to TypeScript.
///
/// # Arguments
/// - `method` — HTTP verb string literal, e.g. `"GET"` (required).
/// - `path`   — URL path relative to `/api/`, e.g. `"measurement/trends"` (required).
/// - `name`   — TypeScript name override (optional; defaults to the name derived from
///   `method` + `path`, e.g. `"GET"` + `"measurement/trends"` → `"GetMeasurementTrends"`).
/// - `depth`  — Number of leading path segments to strip when forming the actix route path
///   (optional; defaults to `1`). Use `depth = 1` when the handler is registered inside a
///   single scope (e.g. `/measurement`), `depth = 2` for two nested scopes, etc.
///
/// # Generated items
/// 1. The handler function annotated with actix-web's route macro (e.g. `#[actix_web::get("…")]`),
///    turning it into an `HttpServiceFactory` usable via `cfg.service(fn_name)`.
/// 2. `generated/bindings/endpoints/<Name>.ts` — written by a `#[test]` when
///    `cargo test export_endpoint` is run (matches the same naming convention as ts-rs's
///    `cargo test export_bindings`).
/// 3. `generated/api/<Name>.ts` — a thin typed wrapper around `callEndpoint`, also written
///    by a `#[test]` when `cargo test export_endpoint` is run.
#[proc_macro_attribute]
pub fn api_endpoint(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as EndpointArgs);
    let func = parse_macro_input!(item as ItemFn);

    match expand(&args, &func) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Derive a PascalCase TypeScript name from an HTTP method and path.
///
/// The method is capitalised as a word (`"GET"` → `"Get"`), and each path segment is
/// converted to PascalCase (`"measurement/trends"` → `"MeasurementTrends"`). Path
/// parameter placeholders have their braces stripped before conversion (`{id}` → `"Id"`).
///
/// Examples:
/// - `"GET"` + `"measurement/trends"` → `"GetMeasurementTrends"`
/// - `"DELETE"` + `"user/webauthn/credentials/{id}"` → `"DeleteUserWebauthnCredentialsId"`
/// - `"POST"` + `"auth/reset-password/complete"` → `"PostAuthResetPasswordComplete"`
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

#[expect(clippy::too_many_lines, reason = "proc-macro expansion is inherently verbose")]
fn expand(args: &EndpointArgs, func: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let method_str = args.method.value();
    let path_str = args.path.value();

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

    // Resolve the `depth` argument (default 1) and compute the actix route path by stripping
    // that many leading segments from the canonical path.
    let depth: usize = args
        .depth
        .as_ref()
        .map(syn::LitInt::base10_parse::<usize>)
        .transpose()
        .map_err(|e| syn::Error::new(e.span(), format!("#[api_endpoint]: {e}")))?
        .unwrap_or(1);

    let route_path: String = path_str
        .split('/')
        .skip(depth)
        .collect::<Vec<_>>()
        .join("/");

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
                 ({segment_count}) in `\"{path_str}\"`"
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

    // Extract types from the function signature.
    let extracted = extract_types(&func.sig)?;

    // Resolve optional types to concrete syn::Type values.
    let query_ty = extracted.query.as_deref();
    let body_ty = extracted.body.as_deref();
    let path_params_ty = extracted.path_params.as_deref();

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
    )?;

    // Build the API wrapper content (thin callEndpoint wrapper).
    let api_content = ts_gen::generate_api_file(
        &ts_name,
        &api_fn_name,
        extracted.query.is_some(),
        extracted.body.is_some(),
        extracted.path_params.is_some(),
    );

    // Output file paths (relative to CARGO_MANIFEST_DIR, i.e. the workspace root).
    let ts_file = format!("generated/bindings/endpoints/{ts_name}.ts");
    let api_file = format!("generated/api/{ts_name}.ts");

    // Generate the test module name and test function names from the Rust fn name.
    let test_mod_name = Ident::new(&format!("__export_endpoint_{fn_name}"), Span::call_site());
    let test_fn_name = Ident::new(&format!("export_endpoint_{fn_name}"), Span::call_site());
    let test_api_fn_name =
        Ident::new(&format!("export_endpoint_api_{fn_name}"), Span::call_site());

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
                    .expect("create generated/bindings/endpoints dir");
                ::std::fs::write(&out, content)
                    .expect("write endpoint TypeScript binding");
            }

            #[test]
            fn #test_api_fn_name() {
                let content: &str = #api_content;
                let out = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(#api_file);
                ::std::fs::create_dir_all(out.parent().expect("parent dir exists"))
                    .expect("create generated/api dir");
                ::std::fs::write(&out, content)
                    .expect("write API wrapper TypeScript file");
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
            ts_name_from_method_and_path("GET", "measurement/trends"),
            "GetMeasurementTrends"
        );
    }

    #[test]
    fn ts_name_single_segment() {
        assert_eq!(
            ts_name_from_method_and_path("DELETE", "measurement"),
            "DeleteMeasurement"
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
            ts_name_from_method_and_path("GET", "measurement/lastXDays"),
            "GetMeasurementLastXDays"
        );
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
