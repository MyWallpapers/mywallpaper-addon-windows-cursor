import './styles.css'
import type { CanvasAddonMountContext, JsonValue, NativeConnection } from '../generated/mywallpaper-runtime'

export function mount({ layer }: CanvasAddonMountContext): () => void {
  const root = layer.root
  root.classList.add('windows-cursor-root')
  root.innerHTML = `
    <main>
      <div class="pointer" aria-hidden="true"></div>
      <div><span>WINDOWS CURSOR</span><strong>Connecting…</strong><p>The custom pointer is applied to this Windows session.</p></div>
    </main>`
  const panel = required<HTMLElement>('main')
  const pointer = required<HTMLElement>('.pointer')
  const state = required<HTMLElement>('strong')
  let connection: NativeConnection | null = null
  let disposed = false

  const renderSettings = (settings: Record<string, JsonValue>) => {
    panel.style.setProperty('--fill', string(settings['fillColor'], '#7c5cff'))
    panel.style.setProperty('--outline', string(settings['outlineColor'], '#ffffff'))
    panel.style.setProperty('--accent', string(settings['glowColor'], '#39d9ff'))
    pointer.style.opacity = settings['enabled'] === false ? '0.35' : '1'
  }
  renderSettings(layer.deviceSettings.get())
  const stopSettings = layer.deviceSettings.subscribe(renderSettings)
  void connect()

  async function connect(): Promise<void> {
    try {
      // Canvas mounts before the desktop finishes attaching the verified
      // artifact. The SDK's `connect()` promise is the synchronization point;
      // reading `available` once here would incorrectly make that transient
      // startup state permanent.
      const next = await layer.native.companion.connect()
      if (disposed) { next.close(); return }
      connection = next
      next.onStateChange((value) => {
        if (value === 'open') setState('Connected', 'success')
        else if (value === 'reconnecting') setState('Reconnecting…', 'warning')
        else setState('Stopped', 'error')
      })
      next.onMessage(receive)
    } catch (error) {
      setState(error instanceof Error ? error.message : String(error), 'error')
    }
  }

  function receive(payload: JsonValue): void {
    if (!isRecord(payload) || payload['kind'] !== 'cursor.status') return
    if (typeof payload['message'] === 'string') setState(payload['message'], payload['active'] === true ? 'success' : 'neutral')
  }

  function setState(message: string, tone: string): void {
    state.textContent = message
    state.dataset['tone'] = tone
  }

  function required<T extends Element>(selector: string): T {
    const element = root.querySelector<T>(selector)
    if (!element) throw new Error(`Windows Cursor UI is missing ${selector}`)
    return element
  }

  return () => {
    disposed = true
    stopSettings()
    connection?.close()
    root.classList.remove('windows-cursor-root')
    root.replaceChildren()
  }
}

function isRecord(value: JsonValue): value is Record<string, JsonValue> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function string(value: JsonValue | undefined, fallback: string): string {
  return typeof value === 'string' ? value : fallback
}
