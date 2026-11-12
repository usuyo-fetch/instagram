# usuyo-fetch/instagram

Instagram 的获取插件，供 [usuyo](https://github.com/usuyo/usuyo) 调用：读你申请下载的 JSON 数据，把收藏和赞过的帖子列成链接条目。

| 方法 | 清单里的 id | 取得 | 账号 | 层级 | 现状 |
|---|---|---|---|---|---|
| 数据下载（JSON） | `export` | 存过的：收藏、收藏夹里的帖子、赞过的帖子，每条是帖子链接，带作者名和收藏或赞的时间；HTML 形式的下载会提示重新申请 JSON | 不使用（你向 Instagram 申请下载） | `official-export` | 已实现；只做了离线测试，样例手写，没有对着真实下载核对；`saved_collections.json` 的形状没有依据，是猜的 |

文件形状和文字编码参考 [scrollback](https://github.com/meizuflux/scrollback)（MIT，Copyright (c) 2024 meizuflux）；单条帖子的内容由别的插件按链接去取。
