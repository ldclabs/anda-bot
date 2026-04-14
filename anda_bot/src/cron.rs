use anda_core::BoxError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub async fn serve(
    _cancel_token: CancellationToken,
) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
    // TODO
    Ok(tokio::spawn(async { Ok(()) }))
}
