### Settings window

settings-window-title = Settings
settings-search-hint = Search settings (e.g. "font", "key", "projection")
# $query: the words typed, $count: number of matching pages
settings-search-found = { $count ->
    [one] 1 page matches "{ $query }"
   *[other] { $count } pages match "{ $query }"
}

## Tabs at the top

settings-tab-general = General
settings-tab-design = Design
settings-tab-machining = Manufacture

## Page names on the left

settings-page-appearance = Appearance
settings-page-input = Mouse and Keys
settings-page-files = Files
settings-page-render = Graphics
settings-page-units = Units
settings-page-new-doc = New Design
settings-page-arrangement = Layout
settings-page-ai = AI Connection
settings-page-view3d = 3D View

## Appearance page: display language

settings-language = Display language
# $lang: name of the OS language, written in that language (e.g. 日本語)
settings-language-auto = Automatic (follow the OS: { $lang })
settings-language-hint = Language of the on-screen text. Text that has not been translated yet is shown in English (Simplified Chinese, Traditional Chinese and Korean are open for translation)
