/// The macro must emit a compile error when the return type is not one of the
/// recognised wrappers (`Json<ApiResult<T>>`, `CustomizeResponder<Json<ApiResult<T>>>`,
/// or `Either<…>`).
use api_macros::api_endpoint;

// A local `Result` alias with a single generic, as our actix handlers use.
type Result<T> = ::std::result::Result<T, Box<dyn ::std::error::Error>>;

struct SomeRandomType;

#[api_endpoint(method = "GET", path = "test/path")]
async fn my_handler() -> Result<SomeRandomType> {
    Ok(SomeRandomType)
}

fn main() {}
