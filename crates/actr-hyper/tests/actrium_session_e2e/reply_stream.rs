use std::{sync::Arc, time::Duration};

use actr_framework::{Bytes, Context, Dest};
use actr_hyper::ActrRef;
use actr_protocol::{ActorResult, ActrId, DataChunk, PayloadType};
use tokio::sync::Notify;

pub(super) const ROUTE: &str = "echo.stream";
const STREAM_ID: &str = "echo-reply";
const REPLY: &[u8] = b"background reply over ordinary Relay";

fn chunk(payload: &'static [u8]) -> DataChunk {
    DataChunk {
        stream_id: STREAM_ID.into(),
        sequence: 1,
        payload: Bytes::from_static(payload),
        ..Default::default()
    }
}

pub(super) fn dispatch<C: Context>(ctx: &C, reply_ready: Arc<Notify>) -> ActorResult<Bytes> {
    let caller = ctx.caller_id().expect("the reply RPC has a caller").clone();
    let ctx = ctx.clone();
    tokio::spawn(async move {
        reply_ready.notified().await;
        ctx.send_data_chunk(
            &Dest::Peer(caller),
            chunk(REPLY),
            PayloadType::StreamReliable,
        )
        .await
        .expect("send the background reply");
    });
    Ok(Bytes::new())
}

pub(super) async fn verify(
    front: &ActrRef,
    other: &ActrRef,
    service: &ActrId,
    reply_ready: &Notify,
) {
    let (send, mut receive) = tokio::sync::mpsc::unbounded_channel();
    let ctx = front.app_context().await;
    ctx.register_stream_from_peer(STREAM_ID.into(), service.clone(), move |chunk, sender| {
        let send = send.clone();
        Box::pin(async move {
            send.send((chunk, sender))
                .expect("the test receiver is open");
            Ok(())
        })
    })
    .unwrap();
    other
        .app_context()
        .await
        .send_data_chunk(
            &Dest::Peer(front.actor_id()),
            chunk(b"unexpected peer"),
            PayloadType::StreamReliable,
        )
        .await
        .unwrap();
    ctx.call_raw(
        &Dest::Peer(service.clone()),
        ROUTE.into(),
        PayloadType::RpcReliable,
        Bytes::new(),
        10_000,
    )
    .await
    .unwrap();
    reply_ready.notify_one();
    let (chunk, sender) = tokio::time::timeout(Duration::from_secs(5), receive.recv())
        .await
        .expect("the Front receives the deferred reply stream")
        .unwrap();
    assert_eq!(sender, *service);
    assert_eq!(chunk.payload, Bytes::from_static(REPLY));
    assert_eq!(chunk.sequence, 1);
    ctx.unregister_stream(STREAM_ID).await.unwrap();
}
