import { open } from '@tauri-apps/plugin-dialog'

export async function pickWorkspace(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择 Markdown 工作区',
  })

  return typeof selected === 'string' ? selected : null
}
