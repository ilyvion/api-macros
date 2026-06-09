use api_macros::api_endpoint;

type Result<T> = ::std::result::Result<T, Box<dyn ::std::error::Error>>;

struct JsonApiResult<T>(::std::marker::PhantomData<T>);

#[api_endpoint(method = "CUSTOM", path = "test/path")]
async fn my_handler() -> Result<JsonApiResult<()>> {
    Ok(JsonApiResult(::std::marker::PhantomData))
}

fn main() {}
