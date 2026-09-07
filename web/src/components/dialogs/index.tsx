// 全局对话框宿主 —— 挂载新建下载 / manifest 前置选择 / HLS 画质 / BT 文件选择 /
// 入站配对核验等对话框；各自内部订阅相应 store 自控开关（见 lib/dialogs.ts、
// lib/ws.ts），无需从外部传 props。

import { BtFilesDialog } from './bt-files'
import { HlsQualityDialog } from './hls-quality'
import { IncomingPairingDialog } from './incoming-pairing'
import { ManifestSelectDialog } from './manifest-select'
import { NewDownloadDialog } from './new-download'
import { RenameTaskDialog } from './rename-task'
import { ResolveVariantDialog } from './resolve-variant'
import { SeedLimitsDialog } from './seed-limits'

export function GlobalDialogs() {
  return (
    <>
      <NewDownloadDialog />
      <ManifestSelectDialog />
      <HlsQualityDialog />
      <ResolveVariantDialog />
      <BtFilesDialog />
      <IncomingPairingDialog />
      <RenameTaskDialog />
      <SeedLimitsDialog />
    </>
  )
}
