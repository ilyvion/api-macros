//! Extract typed roles (query, body, path params, response) from an actix-web handler signature.
//!
//! # Parameter roles
//! - `web::Query<T>`  → query parameters type `T`
//! - `web::Json<T>`   → request body type `T`
//! - `web::Path<T>`   → path parameters type `T`
//! - `web::Data<T>`, `HttpRequest`, and anything else → ignored (injected dependencies)
//!
//! # Return-type roles
//! The outer return type must match one of:
//! - `Result<JsonApiResult<T>>`               i.e. `Result<web::Json<ApiResult<T>>>`
//! - `Result<CustomizedJsonApiResult<T>>`
//! - `Result<Either<JsonApiResult<A>, CustomizedJsonApiResult<B>>>` → response is `A | B`

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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Extract all typed roles from `sig`.
pub(crate) fn extract_types(sig: &Signature) -> Result<ExtractedTypes> {
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

    let response = extract_response_type(sig)?;

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

fn extract_response_type(sig: &Signature) -> Result<ExtractedRole> {
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

    // Try each supported wrapper
    if let Some(t) = try_peel_json_api_result(inner) {
        return Ok(ExtractedRole::Single(t));
    }
    if let Some(t) = try_peel_customized_json_api_result(inner) {
        return Ok(ExtractedRole::Single(t));
    }
    if let Some((a, b)) = try_peel_either(inner) {
        return Ok(ExtractedRole::Union(a, b));
    }

    Err(syn::Error::new_spanned(
        inner,
        "#[api_endpoint]: unrecognised return type; expected \
         `JsonApiResult<T>`, `CustomizedJsonApiResult<T>`, or \
         `Either<JsonApiResult<A>, CustomizedJsonApiResult<B>>`",
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

/// Peel `JsonApiResult<T>` → `T`.
fn try_peel_json_api_result(ty: &Type) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "JsonApiResult" {
        return None;
    }
    single_generic_arg(&seg.arguments)
}

/// Peel `CustomizedJsonApiResult<T>` → `T`.
fn try_peel_customized_json_api_result(ty: &Type) -> Option<Box<Type>> {
    let path = type_to_path(ty)?;
    let seg = last_segment(path)?;
    if seg.ident != "CustomizedJsonApiResult" {
        return None;
    }
    single_generic_arg(&seg.arguments)
}

/// Peel `Either<JsonApiResult<A>, CustomizedJsonApiResult<B>>` → `(A, B)`.
fn try_peel_either(ty: &Type) -> Option<(Box<Type>, Box<Type>)> {
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
    let a = try_peel_json_api_result(left_ty)
        .or_else(|| try_peel_customized_json_api_result(left_ty))?;
    let b = try_peel_json_api_result(right_ty)
        .or_else(|| try_peel_customized_json_api_result(right_ty))?;
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
