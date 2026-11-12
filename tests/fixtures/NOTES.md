# Fixture notes

All files here are hand-written with invented content. NOT MEASURED: nobody involved had a real Instagram data download, so none of this has been compared with one.

- `saved_posts.json`: `saved_saved_media` with `string_map_data` / `Saved on` / `href` + `timestamp`, from the demo data and `src/import/interactions.ts` of [meizuflux/scrollback](https://github.com/meizuflux/scrollback) (MIT, Copyright (c) 2024 meizuflux). The titles carry UTF-8 bytes written as `\u00XX` escapes, the way scrollback's `decodeU8String` assumes Instagram writes them. One entry has no timestamp, one links to a profile, not a post.
- `liked_posts.json`: `likes_media_likes` with `string_list_data` / `href` + `timestamp`, from the same demo data.
- `liked_posts_label_values.json`: the newer `label_values` shape (`URL`, `Owner` > `Username`), from the comment block in scrollback's `interactions.ts`.
- `saved_collections.json`: scrollback does not read this file. Its layout here is a guess (not confirmed by any source): the same item shapes, with entries that have no link for the collections themselves.
- The `_fixture` key in each file is not part of Instagram's format; the plugin ignores it.
- Folder names used in the tests (`your_instagram_activity/saved/`, `your_instagram_activity/likes/`, and the older `saved/`, `likes/`) come from the paths scrollback reads and from the author's memory of older downloads; neither is confirmed.
