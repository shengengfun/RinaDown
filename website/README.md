# FluxDown 官网

基于 Astro + React + Tailwind CSS 构建，部署到 Vercel。

## 环境变量配置

复制 `.env.example` 为 `.env`，填写以下必要变量：

| 变量 | 说明 | 必填 |
|------|------|------|
| `GITHUB_REPO` | 仓库地址（owner/repo） | 是 |
| `GITHUB_TOKEN` | GitHub PAT，需要 `repo` 权限 | 是 |
| `GITHUB_WEBHOOK_SECRET` | Webhook 签名密钥 | 否 |
| `SMTP_HOST/PORT/USER/PASS` | SMTP 邮件配置 | 否 |
| `AFDIAN_USER_ID/TOKEN` | 爱发电 API | 否 |

## 🚀 Project Structure

Inside of your Astro project, you'll see the following folders and files:

```text
/
├── public/
├── src/
│   └── pages/
│       └── index.astro
└── package.json
```

Astro looks for `.astro` or `.md` files in the `src/pages/` directory. Each page is exposed as a route based on its file name.

There's nothing special about `src/components/`, but that's where we like to put any Astro/React/Vue/Svelte/Preact components.

Any static assets, like images, can be placed in the `public/` directory.

## 🧞 Commands

All commands are run from the root of the project, from a terminal:

| Command                   | Action                                           |
| :------------------------ | :----------------------------------------------- |
| `npm install`             | Installs dependencies                            |
| `npm run dev`             | Starts local dev server at `localhost:4321`      |
| `npm run build`           | Build your production site to `./dist/`          |
| `npm run preview`         | Preview your build locally, before deploying     |
| `npm run astro ...`       | Run CLI commands like `astro add`, `astro check` |
| `npm run astro -- --help` | Get help using the Astro CLI                     |

## 👀 Want to learn more?

Feel free to check [our documentation](https://docs.astro.build) or jump into our [Discord server](https://astro.build/chat).
