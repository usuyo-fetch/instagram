# usuyo-fetch/instagram

Instagram 的获取插件，供 [usuyo](https://github.com/usuyo/usuyo) 调用：读你申请下载的 JSON 数据，把收藏和赞过的帖子列成链接条目。

| 方法 | 清单里的 id | 取得 | 账号 | 层级 | 现状 |
|---|---|---|---|---|---|
| 数据下载（JSON） | `export` | 存过的：收藏、收藏夹里的帖子、赞过的帖子，每条是帖子链接，带作者名和收藏或赞的时间；HTML 形式的下载会提示重新申请 JSON | 不使用（你向 Instagram 申请下载） | `official-export` | 已实现；只做了离线测试，样例手写，没有对着真实下载核对；`saved_collections.json` 的形状没有依据，是猜的 |

文件形状和文字编码参考 [scrollback](https://github.com/meizuflux/scrollback)（MIT，Copyright (c) 2024 meizuflux）；单条帖子的内容由别的插件按链接去取。

## 开发

`Cargo.toml` 里的 `usuyo-plugin`、`usuyo-plugin-kit` 是对 `https://github.com/usuyo/usuyo` 的 git 依赖，钉在固定的 `rev`。

本地开发时用旁边 `usuyo/` 的工作区代替它们：`usuyo-fetch/.cargo/config.toml`（不进任何插件仓库）写了下面的覆盖，cargo 从当前目录往上找这个文件，在插件文件夹里跑 cargo 就生效；从别处跑要加 `--config <这个文件>`。

```toml
[patch."https://github.com/usuyo/usuyo"]
usuyo-plugin = { path = "../usuyo/crates/usuyo-plugin" }
usuyo-plugin-kit = { path = "../usuyo/crates/usuyo-plugin-kit" }
```

`Cargo.lock` 记的是覆盖后的路径形式，所以 CI 不加 `--locked` 构建，cargo 只重新锁这两个 usuyo crate。
