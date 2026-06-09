/**
 * HTTP verb literals — used as the method type in {@link EndpointSpec}.
 */
export type HttpMethod =
    | 'GET'
    | 'POST'
    | 'PUT'
    | 'PATCH'
    | 'DELETE'
    | 'HEAD'
    | 'OPTIONS';

/**
 * Typed descriptor for an API endpoint.
 *
 * The four type parameters are **phantom** — they exist only for TypeScript
 * inference at call sites and are not stored at runtime. The optional
 * `_phantom` field prevents TypeScript from collapsing structurally identical
 * instantiations into the same type (which would break inference).
 *
 * @typeParam Q — query-string type (`never` if the endpoint has no query params)
 * @typeParam B — request-body type (`never` if the endpoint has no body)
 * @typeParam P — path-params type (`never` if the endpoint has no path params)
 * @typeParam R — response-data type
 */
export interface EndpointSpec<Q, B, P, R> {
    readonly method: HttpMethod;
    readonly path: string;
    readonly _phantom?: readonly [Q, B, P, R];
}
