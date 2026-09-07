// 行内小转圈。
//
// 投递要走真实网络（超时 10s、失败还重试 3 次）。按钮点下去到结果回来之间
// 必须有动静，否则用户只会再点一次 —— 而每一次点击都是一条真实的对外请求。

export function Spinner({ size = 11 }: { size?: number }) {
  return (
    <svg
      className="flex-shrink-0 animate-spin"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden
    >
      <path
        d="M12 3a9 9 0 1 0 9 9"
        stroke="currentColor"
        strokeWidth="2.6"
        strokeLinecap="round"
      />
    </svg>
  )
}
