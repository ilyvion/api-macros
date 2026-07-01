# api-macros

Procedural macro for exporting actix-web API endpoint contracts as TypeScript.

## Usage

```rust
#[api_endpoint(method = "GET", path = "users/profile")]
pub(crate) async fn get_user_profile(
    pool: web::Data<DbPool>,
    request: HttpRequest,
    filter: web::Query<FilterQuery>,
) -> Result<Json<ApiResult<ProfileResponse>>> {
    // …
}
```

Running `cargo test export_endpoint` writes three files per handler (plus one shared file):

- `<APIM_EXPORT_DIR>/<APIM_ENDPOINTS_PATH>/<Name>.ts` — typed spec object consumed by `callEndpoint`
- `<APIM_EXPORT_DIR>/<APIM_API_PATH>/<Name>.ts` — thin async wrapper that calls `callEndpoint` with the right types
- `<APIM_EXPORT_DIR>/<APIM_ENDPOINTS_PATH>/EndpointSpec.ts` — shared `EndpointSpec` and `HttpMethod` types (same content every time; safe to write from multiple handlers)

## Required configuration

Set the following in your project's `.cargo/config.toml` under `[env]`.

### `APIM_CALL_ENDPOINT_MODULE` (required)

The TypeScript module path from which `callEndpoint` is imported. Used verbatim in the
generated `import` statement:

```typescript
import { callEndpoint } from 'client/services/http';
```

Omitting this variable is a **compile error**.

```toml
[env]
APIM_CALL_ENDPOINT_MODULE = "client/services/http"
```

### `APIM_CALL_ENDPOINT_NAME` (optional, default: `callEndpoint`)

The name of the function exported by `APIM_CALL_ENDPOINT_MODULE`. Override this if your
module exports it under a different name:

```toml
[env]
APIM_CALL_ENDPOINT_NAME = "fetchEndpoint"
```

### `APIM_RESULT_TYPE` (optional, default: `ApiResult`)

The name of the response-wrapper type used in generated return-type annotations and imports.

```toml
[env]
APIM_RESULT_TYPE = "ApiResult"
```

### `APIM_RESULT_PATH` (optional, default: `bindings/ApiResult`)

The TypeScript module path from which `APIM_RESULT_TYPE` is imported:

```toml
[env]
APIM_RESULT_PATH = "bindings/ApiResult"
```

### `APIM_EXPORT_DIR` (optional, default: `generated`)

Root directory for all generated output files, relative to `CARGO_MANIFEST_DIR`:

```toml
[env]
APIM_EXPORT_DIR = "frontend/src/generated"
```

### `APIM_ENDPOINTS_PATH` (optional, default: `bindings/endpoints`)

Sub-path under `APIM_EXPORT_DIR` where endpoint binding files are written. Also used as the
TypeScript module path prefix when the generated API wrappers import spec files:

```toml
[env]
APIM_ENDPOINTS_PATH = "bindings/endpoints"
```

### `APIM_API_PATH` (optional, default: `api`)

Sub-path under `APIM_EXPORT_DIR` where generated API wrapper files are written:

```toml
[env]
APIM_API_PATH = "api"
```

### `APIM_DEPTH_DEFAULT` (optional, default: `1`)

Default value for the `depth` macro argument when it is not specified in the attribute.
Useful when all (or most) handlers in a project share the same scope nesting level:

```toml
[env]
APIM_DEPTH_DEFAULT = "2"
```

### `APIM_MODELS_PATH` (optional, default: `..`)

Relative TypeScript module path used as the parent directory when
importing model types referenced by an endpoint's query/body/path-params/response roles
(e.g. `import type { Foo } from "{APIM_MODELS_PATH}/Foo"`). Change this if model files don't
live one directory above the generated endpoint binding files:

```toml
[env]
APIM_MODELS_PATH = "../models"
```

### `APIM_JSON_API_RESULT_ALIAS` (optional, default: `""`)

If non-empty, this type name is accepted in handler return types as an alias for
`Json<{APIM_RESULT_TYPE}<T>>`. Useful for codebases that define a type alias such as
`type JsonApiResult<T> = Json<ApiResult<T>>`:

```toml
[env]
APIM_JSON_API_RESULT_ALIAS = "JsonApiResult"
```

### `APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS` (optional, default: `""`)

If non-empty, accepted as an alias for `CustomizeResponder<Json<{APIM_RESULT_TYPE}<T>>>`:

```toml
[env]
APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS = "CustomizedJsonApiResult"
```

### `APIM_UNWRAPPED_RESPONSE` (optional, default: `false`)

When set to `1`, `true`, or `yes`, switches the macro to **unwrapped mode**. In this mode:

- Handler return types must omit the `APIM_RESULT_TYPE` layer: `Result<Json<T>>` instead of
  `Result<Json<ApiResult<T>>>`.
- Generated TypeScript wrappers return `Promise<T>` directly instead of `Promise<ApiResult<T>>`.
- The `APIM_RESULT_TYPE` import is omitted from generated API wrapper files entirely.

```toml
[env]
APIM_UNWRAPPED_RESPONSE = "true"
```

This is useful when the consuming project handles API errors at a lower layer (e.g. in the
`callEndpoint` implementation itself) and callers always receive the successful response type
directly.

## Required export from `APIM_CALL_ENDPOINT_MODULE`

### Wrapped mode (default)

The module must export a function compatible with the following signature.
`ApiResult<R>` (or whatever `APIM_RESULT_TYPE` is set to) is imported by the generated
files from `APIM_RESULT_PATH`.

```typescript
export declare function callEndpoint<Q, B, P, R>(
    spec: EndpointSpec<Q, B, P, R>,
    args?: {
        query?: Q;
        body?: B;
        pathParams?: P;
    },
    options?: Omit<RequestInit, 'method'>,
): Promise<ApiResult<R>>;
```

`callEndpoint` should only throw on network or parse failures. HTTP-level errors
(non-2xx responses) must be returned inside `ApiResult` rather than thrown, so
callers can handle them without try/catch.

The `options` parameter is forwarded from the generated wrapper and should be
merged with whatever `RequestInit` options the implementation applies by default
(e.g. credentials, base URL, auth headers). Callers use it to pass per-request
overrides such as `signal` for abort control.

### Unwrapped mode (`APIM_UNWRAPPED_RESPONSE = true`)

The required signature changes so the return type is the raw response type `R`:

```typescript
export declare function callEndpoint<Q, B, P, R>(
    spec: EndpointSpec<Q, B, P, R>,
    args?: {
        query?: Q;
        body?: B;
        pathParams?: P;
    },
    options?: Omit<RequestInit, 'method'>,
): Promise<R>;
```

In this mode, `callEndpoint` is responsible for throwing (or otherwise surfacing) HTTP errors
itself, since there is no wrapper to carry them back to the caller.

## `EndpointSpec` and `HttpMethod`

These types are defined in `EndpointSpec.ts`, which ships with this crate and is copied
verbatim into `<APIM_EXPORT_DIR>/<APIM_ENDPOINTS_PATH>/` by the generated tests. You do
not need to author or maintain them yourself.

```typescript
export type HttpMethod =
    | 'GET'
    | 'POST'
    | 'PUT'
    | 'PATCH'
    | 'DELETE'
    | 'HEAD'
    | 'OPTIONS';

export interface EndpointSpec<Q, B, P, R> {
    readonly method: HttpMethod;
    readonly path: string;
    readonly _phantom?: readonly [Q, B, P, R];
}
```

The generated endpoint binding files import `EndpointSpec` from `./EndpointSpec` (a relative
import co-located with the generated files).

## Macro arguments

| Argument | Required | Description                                                                                       |
| -------- | -------- | ------------------------------------------------------------------------------------------------- |
| `method` | yes      | HTTP verb, e.g. `"GET"`, `"POST"`                                                                 |
| `path`   | yes      | URL path relative to the API root, e.g. `"users/profile"`                                         |
| `name`   | no       | Override the generated PascalCase TypeScript name (defaults to method + path segments)            |
| `depth`  | no       | Number of leading path segments to strip for the actix route path (default: `APIM_DEPTH_DEFAULT`) |

## `depth` and actix scope nesting

The macro emits `#[actix_web::get("…")]` (or the appropriate verb) on the handler function,
making it an `HttpServiceFactory` registrable with `cfg.service(fn_name)`. The path passed to
that actix attribute must be relative to the scope the handler is registered under — not the
full canonical path.

`depth` controls how many leading segments are stripped from `path` to form the actix
route path.

```
path = "users/profile"
  depth = 1  →  actix route = "profile"     (registered under /users scope)
  depth = 2  →  actix route = ""            (scope root of /users/profile)

path = "user/info"
  depth = 1  →  actix route = "info"        (registered under /user scope)
  depth = 2  →  actix route = ""            (scope root of /user/info)
```

The TypeScript path constant always uses `path` verbatim, regardless of `depth`.

## Function signature conventions

The macro inspects the handler's parameter list to determine which roles are present:

- `web::Query<T>` → query-string type (`T`)
- `web::Json<T>` → request-body type (`T`)
- `web::Path<T>` → path-parameter type (`T`)

The return type must be one of the forms below, depending on `APIM_UNWRAPPED_RESPONSE`.

**Wrapped mode (default):**

- `Result<Json<ApiResult<T>>>` — standard JSON response
- `Result<CustomizeResponder<Json<ApiResult<T>>>>` — customized JSON response
- `Result<Either<L, R>>` where `L` and `R` are either of the above → union response `T_L | T_R`

(`ApiResult` here refers to whatever `APIM_RESULT_TYPE` is set to.)

If `APIM_JSON_API_RESULT_ALIAS` or `APIM_CUSTOMIZERESPONDER_JSON_API_RESULT_ALIAS` are set,
those names are also accepted.

**Unwrapped mode (`APIM_UNWRAPPED_RESPONSE = true`):**

- `Result<Json<T>>` — standard JSON response
- `Result<CustomizeResponder<Json<T>>>` — customized JSON response
- `Result<Either<L, R>>` where `L` and `R` are either of the above → union response `T_L | T_R`

Generated API wrapper functions follow the argument order: `pathParams` first,
then `body`, then `query` (optional), then `options` (optional). Required
parameters precede optional ones; `options` is always last so callers can pass
per-request `RequestInit` overrides (e.g. `signal`) without touching the other
arguments.
