//! Parsing of `#[api_endpoint(method = "...", path = "...", name = "...", depth = N)]` arguments.

use proc_macro2::Span;
use syn::{
    Expr, ExprLit, Ident, Lit, LitInt, LitStr, Result, Token,
    parse::{Parse, ParseStream},
};

/// Parsed arguments from the `#[api_endpoint(...)]` attribute.
pub(crate) struct EndpointArgs {
    /// HTTP method string, e.g. `"GET"`.
    pub(crate) method: LitStr,
    /// URL path relative to `/api/`, e.g. `"users/profile"`.
    pub(crate) path: LitStr,
    /// Optional override for the generated TypeScript name (`PascalCase`).
    /// Defaults to the name derived from `method` + `path` (e.g. `"GET"` + `"foo/bar"` →
    /// `"GetFooBar"`).
    pub(crate) name: Option<LitStr>,
    /// Number of leading path segments to strip when emitting the actix route path.
    /// Defaults to `1`, which strips one segment (e.g. `"user/info"` → `"info"` inside a
    /// `/user` scope). Set higher for handlers registered under nested scopes.
    pub(crate) depth: Option<LitInt>,
    /// Optional name of a Rust type (e.g. `"MyFieldErrorEnum"`) describing the per-message
    /// field tag emitted on `success: false` responses. When set, the generated TypeScript
    /// binding exports a `{Name}FieldErrors` alias for it and includes it as the response
    /// wrapper's field-errors type parameter.
    pub(crate) field_errors: Option<LitStr>,
}

impl Parse for EndpointArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut method: Option<LitStr> = None;
        let mut path: Option<LitStr> = None;
        let mut name: Option<LitStr> = None;
        let mut depth: Option<LitInt> = None;
        let mut field_errors: Option<LitStr> = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _eq: Token![=] = input.parse()?;

            match key.to_string().as_str() {
                "method" => method = Some(parse_str_lit(input, &key)?),
                "path" => path = Some(parse_str_lit(input, &key)?),
                "name" => name = Some(parse_str_lit(input, &key)?),
                "depth" => depth = Some(parse_int_lit(input, &key)?),
                "field_errors" => field_errors = Some(parse_str_lit(input, &key)?),
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!(
                            "unknown api_endpoint argument `{other}`; \
                             expected `method`, `path`, `name`, `depth`, or `field_errors`"
                        ),
                    ));
                }
            }

            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        let method = method.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "api_endpoint requires `method = \"...\"`",
            )
        })?;
        let path = path.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "api_endpoint requires `path = \"...\"`")
        })?;

        Ok(Self {
            method,
            path,
            name,
            depth,
            field_errors,
        })
    }
}

fn parse_str_lit(input: ParseStream<'_>, key: &Ident) -> Result<LitStr> {
    let expr: Expr = input.parse()?;
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s),
        other => Err(syn::Error::new_spanned(
            other,
            format!("value of `{key}` must be a string literal"),
        )),
    }
}

fn parse_int_lit(input: ParseStream<'_>, key: &Ident) -> Result<LitInt> {
    let expr: Expr = input.parse()?;
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => Ok(i),
        other => Err(syn::Error::new_spanned(
            other,
            format!("value of `{key}` must be an integer literal"),
        )),
    }
}
