// Generated from engine/app_protocol.rs. Run ANDA_EXPORT_PROTOCOL=1 RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_protocol_types --bin anda.
export type SubmissionState = "accepted" | "completed" | "failed" | "unknown";
export type SubmissionReceipt = { requestId: string, source: string, state: SubmissionState, result?: unknown, error?: string, };
export type AppCapabilities = { stateInvalidation: boolean, submissionReceipts: boolean, };
export type AppInitialize = { protocolVersion: number, instanceId: string, capabilities: AppCapabilities, };
export type StateChanged = { instanceId: string, revision: string, };
export type AppSubmit = { requestId: string, input: unknown, };
export type SubmissionRead = { source: string, requestId: string, };
