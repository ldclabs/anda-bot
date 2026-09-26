export interface UpdateCoordinator {
  managed: boolean
  wasRunning: boolean
  begin(): Promise<{ token: string; ready: boolean }>
  renew(token: string): Promise<{ token: string; ready: boolean }>
  release(token: string): Promise<void>
  stop(token: string): Promise<void>
  saveIntent(): Promise<void>
  install(): void
  recover(): Promise<void>
  wait(): Promise<void>
}
/** Keep all native/daemon effects behind an ordered, testable coordinator. */
export async function installCoordinated(coordinator: UpdateCoordinator): Promise<void> {
  let lease: string | undefined
  let stopping = false
  try {
    if (coordinator.managed && coordinator.wasRunning) {
      let state = await coordinator.begin()
      lease = state.token
      // Recheck after the pause has propagated through already-admitted input
      // queues and completion hooks, including an initially idle runtime.
      await coordinator.wait()
      state = await coordinator.renew(lease)
      for (let attempt = 0; !state.ready && attempt < 18; attempt++) {
        await coordinator.wait()
        state = await coordinator.renew(lease)
      }
      if (!state.ready)
        throw new Error(
          'Tasks are still active. The update is downloaded; install it after they finish.'
        )
    }
    await coordinator.saveIntent()
    if (lease) {
      stopping = true
      await coordinator.stop(lease)
    }
    coordinator.install()
  } catch (error) {
    if (lease && !stopping) await coordinator.release(lease).catch(() => {})
    if (stopping) await coordinator.recover().catch(() => {})
    throw error
  }
}
