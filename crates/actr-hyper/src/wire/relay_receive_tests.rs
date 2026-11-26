use super::*;

#[tokio::test]
async fn relay_accepts_a_reliable_burst_while_the_consumer_is_stalled() {
    let mut fixture = Fixture::new().await;
    let (request, dial) = fixture.start_dial(fixture.remote.clone()).await;
    fixture.connected(Some(request)).await.unwrap();
    let mut remote = fixture.handshake().await;
    let (_, connection) = dial.await.unwrap().unwrap();
    let budget = Arc::new(Semaphore::new(QUEUE_BYTES));
    let channel = fixture.client.channels.read().await[&42].clone();

    // Model a burst arriving before the dispatcher gets another turn to drain.
    tokio::task::unconstrained(async {
        for sequence in 0..64u8 {
            let frame = [PayloadType::StreamReliable as u8, 0, 0, 0, 1, sequence];
            let record = Bytes::from(one_record(remote.send(&frame, fixture.now).unwrap()));
            fixture
                .client
                .receive_secure(42, record, &fixture.transport, &budget)
                .await
                .unwrap_or_else(|error| panic!("burst frame {sequence} was rejected: {error}"));
        }
    })
    .await;

    assert!(connection.is_connected());
    let reserved = 64 * MIN_RECEIVE_RECORD_CHARGE;
    assert_eq!(budget.available_permits(), QUEUE_BYTES - reserved);
    assert_eq!(
        channel.receive_budget.available_permits(),
        CHANNEL_RECEIVE_BYTES - reserved
    );
}

#[tokio::test]
async fn exhausted_channel_budget_leaves_shared_capacity_available() {
    let mut fixture = Fixture::new().await;
    let (request, dial) = fixture.start_dial(fixture.remote.clone()).await;
    fixture.connected(Some(request)).await.unwrap();
    let mut remote = fixture.handshake().await;
    dial.await.unwrap().unwrap();

    let budget = Arc::new(Semaphore::new(QUEUE_BYTES));
    let channel = fixture.client.channels.read().await[&42].clone();
    let reserved = channel
        .receive_budget
        .clone()
        .try_acquire_many_owned(CHANNEL_RECEIVE_BYTES as u32)
        .unwrap();
    let frame = [PayloadType::StreamReliable as u8, 0, 0, 0, 0];
    let record = Bytes::from(one_record(remote.send(&frame, fixture.now).unwrap()));
    let result = fixture
        .client
        .receive_secure(42, record.clone(), &fixture.transport, &budget)
        .await;

    assert!(result.is_err());
    assert_eq!(budget.available_permits(), QUEUE_BYTES);
    drop(reserved);
    fixture
        .client
        .receive_secure(42, record, &fixture.transport, &budget)
        .await
        .unwrap();
}
