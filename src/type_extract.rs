//! Extract typed roles (query, body, path params, response) from an actix-web handler signature.
//!
//! # Parameter roles
//! - `web::Query<T>`  → query parameters type `T`
//! - `web::Json<T>`   → request body type `T`
//! - `web::Path<T>`   → path parameters type `T`
//! - `web::Data<T>`, `HttpRequest`, and anything else → ignored (injected dependencies)
//!
//! # Return-type roles
//! The accepted outer return type depends on `APIM_UNWRAPPED_RESPONSE`.
//!
//! **Wrapped mode (default):**
//! - `Result<Json<{result_type}<T>>>` — standard JSON response
//! - `Result<CustomizeResponder<Json<{result_type}<T>>>>` — customized JSON response
//! - `Result<Either<L, R>>` where `L` and `R` are either of the above → response is `L_T | R_T`
//!
//! Where `result_type` is controlled by the `APIM_RESULT_TYPE` environment variable
//! (default: `ApiResult`). If `APIM_JSON_API_RESULT_ALIAS` or
//! `APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS` are set, those names are also accepted
//! as single-generic-argument aliases for the respective canonical forms.
//!
//! **Unwrapped mode (`APIM_UNWRAPPED_RESPONSE = true`):**
//! - `Result<Json<T>>` — standard JSON response
//! - `Result<CustomizeResponder<Json<T>>>` — customized JSON response
//! - `Result<Either<L, R>>` where `L` and `R` are either of the above → response is `L_T | R_T`

use proc_macro2::Span;
use syn::{
    FnArg, GenericArgument, PatType, PathArguments, Result, ReturnType, Signature, Type, TypePath,
    TypeTuple,
};

// ---------------------------------------------------------------------------
// Public extracted types
// ---------------------------------------------------------------------------

/// A role extracted from the function signature.
#[derive(Clone)]
pub(crate) enum ExtractedRole {
    /// A single concrete type.
    Single(Box<Type>),
    /// A union of two types (from `Either<A, B>`).
    Union(Box<Type>, Box<Type>),
}

/// All type information extracted from a handler signature.
pub(crate) struct ExtractedTypes {
    pub(crate) query: Option<Box<Type>>,
    pub(crate) body: Option<Box<Type>>,
    pub(crate) path_params: Option<Box<Type>>,
    pub(crate) response: ExtractedRole,
}

/// Configuration for type extraction, derived from environment variables.
pub(crate) struct ExtractConfig<'a> {
    /// Name of the result wrapper type (from `APIM_RESULT_TYPE`), e.g. `"ApiResult"`.
    pub(crate) result_type: &'a str,
    /// Optional alias accepted in place of `Json<result_type<T>>`
    /// (from `APIM_JSON_API_RESULT_ALIAS`).
    pub(crate) json_alias: Option<&'a str>,
    /// Optional alias accepted in place of `CustomizeResponder<Json<result_type<T>>>`
    /// (from `APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS`).
    pub(crate) customized_alias: Option<&'a str>,
    /// When `true` (from `APIM_UNWRAPPED_RESPONSE`), handlers return `Result<Json<T>>` without
    /// any wrapper, and the accepted forms are `Json<T>` and `CustomizeResponder<Json<T>>`.
    pub(crate) unwrapped_response: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Extract all typed roles from `sig`.
pub(crate) fn extract_types(sig: &Signature, config: &ExtractConfig<'_>) -> Result<ExtractedTypes> {
    let mut query: Option<Box<Type>> = None;
    let mut body: Option<Box<Type>> = None;
    let mut path_params: Option<Box<Type>> = None;

    for arg in &sig.inputs {
        let FnArg::Typed(PatType { ty, .. }) = arg else {
            continue; // skip `self`
        };

        if let Some(inner) = try_extract_web_wrapper(ty, "Query") {
            query = Some(inner);
        } else if let Some(inner) = try_extract_web_wrapper(ty, "Json") {
            body = Some(inner);
        } else if let Some(inner) = try_extract_web_wrapper(ty, "Path") {
            path_params = Some(inner);
        }
        // web::Data<T>, HttpRequest, etc. — ignored
    }

    let response = extract_response_type(sig, config)?;

    Ok(ExtractedTypes {
        query,
        body,
        path_params,
        response,
    })
}

// ---------------------------------------------------------------------------
// Parameter extraction helpers
// ---------------------------------------------------------------------------

/// If `ty` is `web::Wrapper<T>` or just `Wrapper<T>`, return the inner `T`.
fn try_extract_web_wrapper(ty: &Type, wrapper: &str) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != wrapper {
        return None;
    }
    single_generic_arg(&seg.arguments)
}

// ---------------------------------------------------------------------------
// Return-type extraction
// ---------------------------------------------------------------------------

fn extract_response_type(sig: &Signature, config: &ExtractConfig<'_>) -> Result<ExtractedRole> {
    let ReturnType::Type(_, ret_ty) = &sig.output else {
        return Err(syn::Error::new(
            sig.fn_token.span,
            "#[api_endpoint] requires an explicit return type",
        ));
    };

    // Peel `Result<INNER>` or `crate::models::Result<INNER>` etc.
    let inner = peel_result(ret_ty).ok_or_else(|| {
        syn::Error::new_spanned(
            ret_ty.as_ref(),
            "#[api_endpoint]: return type must be `Result<…>`",
        )
    })?;

    if config.unwrapped_response {
        // Unwrapped mode: handlers return Result<Json<T>> with no intermediate wrapper.
        if let Some(t) = try_peel_json_raw(inner) {
            return Ok(ExtractedRole::Single(t));
        }
        if let Some(t) = try_peel_customized_raw(inner) {
            return Ok(ExtractedRole::Single(t));
        }
        if let Some((a, b)) = try_peel_either_raw(inner) {
            return Ok(ExtractedRole::Union(a, b));
        }
        return Err(syn::Error::new_spanned(
            inner,
            "#[api_endpoint]: unrecognised return type; in unwrapped mode expected \
             `Json<T>`, `CustomizeResponder<Json<T>>`, \
             or `Either<L, R>` where L and R are either of the above",
        ));
    }

    // Wrapped mode (default): handlers return Result<Json<{result_type}<T>>>.
    if let Some(t) = try_peel_json_result(inner, config) {
        return Ok(ExtractedRole::Single(t));
    }
    if let Some(t) = try_peel_customized_result(inner, config) {
        return Ok(ExtractedRole::Single(t));
    }
    if let Some((a, b)) = try_peel_either(inner, config) {
        return Ok(ExtractedRole::Union(a, b));
    }

    let rt = config.result_type;
    let json_alias_note = config
        .json_alias
        .map_or(String::new(), |a| format!(", `{a}<T>`"));
    let cust_alias_note = config
        .customized_alias
        .map_or(String::new(), |a| format!(", `{a}<T>`"));
    Err(syn::Error::new_spanned(
        inner,
        format!(
            "#[api_endpoint]: unrecognised return type; expected \
             `Json<{rt}<T>>`{json_alias_note}, \
             `CustomizeResponder<Json<{rt}<T>>>`{cust_alias_note}, \
             or `Either<L, R>` where L and R are either of the above"
        ),
    ))
}

/// Peel `Result<T>` → `T`. Accepts the last path segment being `Result`.
fn peel_result(ty: &Type) -> Option<&Type> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "Result" {
        return None;
    }
    single_generic_arg_ref(&seg.arguments)
}

/// Match a single-arg wrapper by exact name: `{name}<T>` → `T`.
fn try_peel_by_name(ty: &Type, name: &str) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != name {
        return None;
    }
    single_generic_arg(&seg.arguments)
}

/// Peel the canonical `Json<{result_type}<T>>` form → `T`.
fn try_peel_json_result_canonical(ty: &Type, result_type: &str) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "Json" {
        return None;
    }
    let inner = single_generic_arg_ref(&seg.arguments)?;
    let inner_path = type_to_path(inner)?;
    let inner_seg = last_segment(inner_path)?;
    if inner_seg.ident != result_type {
        return None;
    }
    single_generic_arg(&inner_seg.arguments)
}

/// Peel `Json<{result_type}<T>>` → `T`, or the configured alias if it matches.
fn try_peel_json_result(ty: &Type, config: &ExtractConfig<'_>) -> Option<Box<Type>> {
    if let Some(alias) = config.json_alias
        && let Some(t) = try_peel_by_name(ty, alias)
    {
        return Some(t);
    }
    try_peel_json_result_canonical(ty, config.result_type)
}

/// Peel `CustomizeResponder<Json<{result_type}<T>>>` → `T`, or the configured alias if it matches.
fn try_peel_customized_result(ty: &Type, config: &ExtractConfig<'_>) -> Option<Box<Type>> {
    if let Some(alias) = config.customized_alias
        && let Some(t) = try_peel_by_name(ty, alias)
    {
        return Some(t);
    }
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "CustomizeResponder" {
        return None;
    }
    let inner = single_generic_arg_ref(&seg.arguments)?;
    try_peel_json_result_canonical(inner, config.result_type)
}

/// Peel `Either<L, R>` → `(T_L, T_R)` where each branch is a recognised response wrapper.
fn try_peel_either(ty: &Type, config: &ExtractConfig<'_>) -> Option<(Box<Type>, Box<Type>)> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "Either" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let generics: Vec<&GenericArgument> = args.args.iter().collect();
    if generics.len() != 2 {
        return None;
    }
    let GenericArgument::Type(left_ty) = generics[0] else {
        return None;
    };
    let GenericArgument::Type(right_ty) = generics[1] else {
        return None;
    };
    let a = try_peel_json_result(left_ty, config)
        .or_else(|| try_peel_customized_result(left_ty, config))?;
    let b = try_peel_json_result(right_ty, config)
        .or_else(|| try_peel_customized_result(right_ty, config))?;
    Some((a, b))
}

// ---------------------------------------------------------------------------
// Unwrapped-mode peel helpers (no result_type layer)
// ---------------------------------------------------------------------------

/// Peel `Json<T>` → `T` (unwrapped mode).
fn try_peel_json_raw(ty: &Type) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "Json" {
        return None;
    }
    single_generic_arg(&seg.arguments)
}

/// Peel `CustomizeResponder<Json<T>>` → `T` (unwrapped mode).
fn try_peel_customized_raw(ty: &Type) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "CustomizeResponder" {
        return None;
    }
    let inner = single_generic_arg_ref(&seg.arguments)?;
    try_peel_json_raw(inner)
}

/// Peel `Either<L, R>` → `(T_L, T_R)` in unwrapped mode.
fn try_peel_either_raw(ty: &Type) -> Option<(Box<Type>, Box<Type>)> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "Either" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let generics: Vec<&GenericArgument> = args.args.iter().collect();
    if generics.len() != 2 {
        return None;
    }
    let GenericArgument::Type(left_ty) = generics[0] else {
        return None;
    };
    let GenericArgument::Type(right_ty) = generics[1] else {
        return None;
    };
    let a = try_peel_json_raw(left_ty).or_else(|| try_peel_customized_raw(left_ty))?;
    let b = try_peel_json_raw(right_ty).or_else(|| try_peel_customized_raw(right_ty))?;
    Some((a, b))
}

// ---------------------------------------------------------------------------
// Low-level syn helpers
// ---------------------------------------------------------------------------

const fn type_to_path(ty: &Type) -> Option<&TypePath> {
    match ty {
        Type::Path(p) => Some(p),
        _ => None,
    }
}

fn last_segment(path: &TypePath) -> Option<&syn::PathSegment> {
    path.path.segments.last()
}

/// Extract the single generic type argument from `AngleBracketed<T>`, returning a boxed clone.
fn single_generic_arg(args: &PathArguments) -> Option<Box<Type>> {
    let PathArguments::AngleBracketed(ab) = args else {
        return None;
    };
    if ab.args.len() != 1 {
        return None;
    }
    match &ab.args[0] {
        GenericArgument::Type(t) => Some(Box::new(t.clone())),
        _ => None,
    }
}

/// Like `single_generic_arg` but returns a reference instead of a clone.
fn single_generic_arg_ref(args: &PathArguments) -> Option<&Type> {
    let PathArguments::AngleBracketed(ab) = args else {
        return None;
    };
    if ab.args.len() != 1 {
        return None;
    }
    match &ab.args[0] {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helper: unit type `()`
// ---------------------------------------------------------------------------

/// Return `true` if the type is the unit tuple `()`.
pub(crate) fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(TypeTuple { elems, .. }) if elems.is_empty())
}

/// A synthetic unit type, used as a sentinel when needed.
#[expect(dead_code, reason = "reserved for future use in expand()")]
pub(crate) fn unit_type() -> Type {
    Type::Tuple(TypeTuple {
        paren_token: syn::token::Paren(Span::call_site()),
        elems: syn::punctuated::Punctuated::new(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_sig(src: &str) -> Signature {
        let func: syn::ItemFn = syn::parse_str(src).expect("valid fn");
        func.sig
    }

    fn wrapped_config() -> ExtractConfig<'static> {
        ExtractConfig {
            result_type: "ApiResult",
            json_alias: None,
            customized_alias: None,
            unwrapped_response: false,
        }
    }

    fn unwrapped_config() -> ExtractConfig<'static> {
        ExtractConfig {
            result_type: "ApiResult",
            json_alias: None,
            customized_alias: None,
            unwrapped_response: true,
        }
    }

    // ------------------------------------------------------------------
    // Unwrapped mode: accepted return type forms
    // ------------------------------------------------------------------

    // These tests use `Result<T>` (single-arg alias form) matching how real actix handlers
    // declare their return type via a local `type Result<T> = std::result::Result<T, Error>`.

    #[test]
    fn unwrapped_json_t_extracts_inner() {
        let sig = parse_sig("async fn h() -> Result<Json<UserInfo>> {}");
        let extracted = extract_types(&sig, &unwrapped_config()).unwrap();
        let ExtractedRole::Single(ty) = extracted.response else {
            panic!("expected Single");
        };
        assert_eq!(quote::quote!(#ty).to_string(), "UserInfo");
    }

    #[test]
    fn unwrapped_customized_json_t_extracts_inner() {
        let sig =
            parse_sig("async fn h() -> Result<CustomizeResponder<Json<UserInfo>>> {}");
        let extracted = extract_types(&sig, &unwrapped_config()).unwrap();
        let ExtractedRole::Single(ty) = extracted.response else {
            panic!("expected Single");
        };
        assert_eq!(quote::quote!(#ty).to_string(), "UserInfo");
    }

    #[test]
    fn unwrapped_either_extracts_union() {
        let sig =
            parse_sig("async fn h() -> Result<Either<Json<TypeA>, Json<TypeB>>> {}");
        let extracted = extract_types(&sig, &unwrapped_config()).unwrap();
        let ExtractedRole::Union(a, b) = extracted.response else {
            panic!("expected Union");
        };
        assert_eq!(quote::quote!(#a).to_string(), "TypeA");
        assert_eq!(quote::quote!(#b).to_string(), "TypeB");
    }

    #[test]
    fn unwrapped_rejects_non_json_inner() {
        // A non-Json inner type has no peel path in unwrapped mode and must fail.
        let sig = parse_sig("async fn h() -> Result<SomeRandomType> {}");
        assert!(
            extract_types(&sig, &unwrapped_config()).is_err(),
            "non-Json inner type must be rejected in unwrapped mode"
        );
    }

    // ------------------------------------------------------------------
    // Wrapped mode: still rejects Json<T> without result_type wrapper
    // ------------------------------------------------------------------

    #[test]
    fn wrapped_rejects_raw_json_t() {
        let sig = parse_sig("async fn h() -> Result<Json<UserInfo>> {}");
        assert!(
            extract_types(&sig, &wrapped_config()).is_err(),
            "Json<T> without ApiResult wrapper must be rejected in wrapped mode"
        );
    }

    #[test]
    fn wrapped_accepts_json_api_result_t() {
        let sig = parse_sig("async fn h() -> Result<Json<ApiResult<UserInfo>>> {}");
        let extracted = extract_types(&sig, &wrapped_config()).unwrap();
        let ExtractedRole::Single(ty) = extracted.response else {
            panic!("expected Single");
        };
        assert_eq!(quote::quote!(#ty).to_string(), "UserInfo");
    }
}
