<script setup lang="ts">
import { useData } from 'vitepress'

// `DOCS_CHANNEL=dev` at build time (a `robots` meta is added too).
const { theme } = useData()
const dev = theme.value.devChannel === true
</script>

<template>
  <div v-if="dev" class="dev-banner">
    This guide follows RustCMS-Core <code>main</code> and describes unreleased changes.
    <a href="https://irisvn.github.io/RustCMS/">Read the guide of the latest release</a>.
  </div>
</template>

<style scoped>
.dev-banner {
  padding: 8px 16px;
  text-align: center;
  font-size: 14px;
  background: var(--vp-c-warning-soft);
  color: var(--vp-c-text-1);
}
.dev-banner a {
  color: var(--vp-c-brand-1);
  text-decoration: underline;
}
</style>
