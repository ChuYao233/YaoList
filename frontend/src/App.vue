<template>
  <router-view />
  <GlobalNotification />
</template>

<script setup>
import GlobalNotification from './components/GlobalNotification.vue'
// 全局逻辑可放这里，如全局样式、全局状态等
</script>

<style lang="css">
html, body, #app {
  height: 100%;
  margin: 0;
  padding: 0;
  background: #f5f6fa;
}

/* 确保主题色变量可用 */
:root {
  --theme-color: #1976d2;
  --theme-color-rgb: 25, 118, 210;
}
</style>
