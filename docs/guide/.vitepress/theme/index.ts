// Default theme with the RustCMS colors (the slide decks' teal and amber) and,
// on the build of `main`, a banner saying the guide is unreleased.
import { h } from 'vue'
import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import DevBanner from './DevBanner.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  Layout: () => h(DefaultTheme.Layout, null, { 'layout-top': () => h(DevBanner) }),
} satisfies Theme
