# FluxDown — Unraid Community Applications 模板

> **重要：本目录必须作为一个独立仓库 `zerx-lab/unraid-templates` 的根推送，
> 不能留在 FluxDown 主仓库里提交给 CA。**

## 为什么要独立仓库

CA 的 live scan 会**递归扫描整个仓库根**，把所有 `.xml` 都当候选 Unraid 模板解析。
FluxDown 主仓库里有 8 个无关 XML（`android/**` 的 AndroidManifest/styles/launch_background、
`linux/runner/**` 的 gresource），会触发 `not_unraid_application: 8` 警告。
Unraid 社区标准做法（ibracorp、digiblur、Josh5 等）都是用一个**只放模板的干净仓库**。

## 目录结构（即独立仓库的根）

```
unraid-templates/            # → zerx-lab/unraid-templates 仓库根
├── ca_profile.xml           # 根目录，必须（<Maintainer> 内含非空 <Profile>）
├── README.md
└── FluxDown/
    └── fluxdown.xml         # 模板；TemplateURL 已指向本仓库 raw 路径
```

## 提交步骤

```bash
# 1. 新建独立仓库 zerx-lab/unraid-templates（GitHub 网页或 gh repo create）
# 2. 把本目录（ca_profile.xml + README.md + FluxDown/）作为仓库根内容 push 到 main
# 3. 到 https://ca.unraid.net/submit 填仓库地址 zerx-lab/unraid-templates
#    live scan 应全绿：ca_profile <Profile> 非空、无 not_unraid_application 警告
```

## 已修正的两处扫描错误

- **ca_profile.xml missing `<Profile>`**：根元素改为 `<Maintainer>`，内含非空 `<Profile>` 文字描述
  （照 Josh5/ibracorp 真实范本）。
- **not_unraid_application: 8**：迁出主仓库到独立干净仓库，scanner 只会看到 `FluxDown/fluxdown.xml`。

## 备注

- `TemplateURL` 已指向 `raw.githubusercontent.com/zerx-lab/unraid-templates/main/FluxDown/fluxdown.xml`。
  若仓库名不用 `unraid-templates`，需同步改此 URL 与 `ca_profile.xml`/`fluxdown.xml` 里的 raw 链接。
- Icon 目前用 SVG。CA 界面对 SVG 支持不稳，建议改用 PNG（可从 `assets/logo/` 导出 256×256 放到本仓库
  `FluxDown/icon.png` 并改 `<Icon>` 链接）。scanner 不因此报错，仅影响显示。
