export interface Lease {
  token: string
  ready: boolean
}
/** One runtime update: the shared `anda` CLI is replaced by its own updater. */
export interface RuntimeUpdate {
  /** The daemon is running and must drain before its binary is replaced. */
  running: boolean
  begin(): Promise<Lease>
  renew(token: string): Promise<Lease>
  release(token: string): Promise<void>
  /** Replaces the binary and brings the daemon back on the new release. */
  install(lease?: string): Promise<void>
  /** Keeps the daemon usable after a failed install. */
  recover(lease?: string): Promise<void>
  wait(): Promise<void>
}
/** Keep all native/daemon effects behind an ordered, testable coordinator. */
export async function installRuntimeUpdate(update: RuntimeUpdate): Promise<void> {
  let lease: string | undefined
  if (update.running) {
    let state = await update.begin()
    lease = state.token
    // Recheck after the pause has propagated through already-admitted input
    // queues and completion hooks, including an initially idle runtime.
    await update.wait()
    state = await update.renew(lease)
    for (let attempt = 0; !state.ready && attempt < 18; attempt++) {
      await update.wait()
      state = await update.renew(lease)
    }
    if (!state.ready) {
      await update.release(lease).catch(() => {})
      throw new Error(
        'Tasks are still active. The update stays downloaded; install it after they finish.'
      )
    }
  }
  try {
    await update.install(lease)
  } catch (error) {
    await update.recover(lease).catch(() => {})
    throw error
  }
}
/** Compares release versions such as `v0.13.0` and `0.13.1`. */
export function isOlderRelease(version: string, than: string): boolean {
  const parts = (value: string) =>
    value
      .replace(/^v/, '')
      .split(/[-+]/)[0]!
      .split('.')
      .map((part) => Number.parseInt(part, 10) || 0)
  const [left, right] = [parts(version), parts(than)]
  for (let index = 0; index < Math.max(left.length, right.length); index++) {
    const difference = (left[index] || 0) - (right[index] || 0)
    if (difference) return difference < 0
  }
  return false
}
