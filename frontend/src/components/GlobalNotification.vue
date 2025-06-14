<template>
  <Teleport to="body">
    <div class="notification-container">
      <TransitionGroup name="notification" tag="div">
        <div
          v-for="notification in notifications"
          :key="notification.id"
          :class="['notification-banner', notification.type]"
          @click="removeNotification(notification.id)"
        >
          <div class="notification-content">
            <div class="notification-icon">
              <svg v-if="notification.type === 'success'" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="20,6 9,17 4,12"/>
              </svg>
              <svg v-else-if="notification.type === 'error'" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10"/>
                <line x1="15" y1="9" x2="9" y2="15"/>
                <line x1="9" y1="9" x2="15" y2="15"/>
              </svg>
              <svg v-else-if="notification.type === 'warning'" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
                <line x1="12" y1="9" x2="12" y2="13"/>
                <line x1="12" y1="17" x2="12.01" y2="17"/>
              </svg>
              <svg v-else width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10"/>
                <path d="M12 16v-4"/>
                <path d="M12 8h.01"/>
              </svg>
            </div>
            <div class="notification-message">{{ notification.message }}</div>
            <button class="notification-close" @click.stop="removeNotification(notification.id)">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue'

const notifications = ref([])
let notificationId = 0

// 添加通知
function addNotification(message, type = 'info', duration = 3000) {
  const id = ++notificationId
  const notification = {
    id,
    message,
    type,
    duration
  }
  
  notifications.value.push(notification)
  
  // 自动移除通知
  if (duration > 0) {
    setTimeout(() => {
      removeNotification(id)
    }, duration)
  }
  
  return id
}

// 移除通知
function removeNotification(id) {
  const index = notifications.value.findIndex(n => n.id === id)
  if (index > -1) {
    notifications.value.splice(index, 1)
  }
}

// 清空所有通知
function clearAll() {
  notifications.value = []
}

// 全局事件监听
function handleGlobalNotification(event) {
  const { message, type, duration } = event.detail
  addNotification(message, type, duration)
}

onMounted(() => {
  // 监听全局通知事件
  window.addEventListener('global-notification', handleGlobalNotification)
})

onUnmounted(() => {
  window.removeEventListener('global-notification', handleGlobalNotification)
})

// 暴露方法给外部使用
defineExpose({
  addNotification,
  removeNotification,
  clearAll
})
</script>

<style scoped>
.notification-container {
  position: fixed;
  top: 20px;
  right: 20px;
  z-index: 10000;
  pointer-events: none;
  max-width: 400px;
  width: 100%;
}

.notification-banner {
  background: white;
  border-radius: 12px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.12);
  margin-bottom: 12px;
  overflow: hidden;
  pointer-events: auto;
  cursor: pointer;
  border-left: 4px solid;
  backdrop-filter: blur(10px);
  transition: all 0.3s ease;
}

.notification-banner:hover {
  transform: translateX(-4px);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.15);
}

.notification-banner.success {
  border-left-color: #4caf50;
  background: linear-gradient(135deg, rgba(76, 175, 80, 0.1), rgba(76, 175, 80, 0.05));
}

.notification-banner.error {
  border-left-color: #f44336;
  background: linear-gradient(135deg, rgba(244, 67, 54, 0.1), rgba(244, 67, 54, 0.05));
}

.notification-banner.warning {
  border-left-color: #ff9800;
  background: linear-gradient(135deg, rgba(255, 152, 0, 0.1), rgba(255, 152, 0, 0.05));
}

.notification-banner.info {
  border-left-color: #2196f3;
  background: linear-gradient(135deg, rgba(33, 150, 243, 0.1), rgba(33, 150, 243, 0.05));
}

.notification-content {
  display: flex;
  align-items: center;
  padding: 16px 20px;
  gap: 12px;
}

.notification-icon {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
}

.notification-banner.success .notification-icon {
  color: #4caf50;
}

.notification-banner.error .notification-icon {
  color: #f44336;
}

.notification-banner.warning .notification-icon {
  color: #ff9800;
}

.notification-banner.info .notification-icon {
  color: #2196f3;
}

.notification-message {
  flex: 1;
  font-size: 14px;
  font-weight: 500;
  color: #333;
  line-height: 1.4;
}

/* 根据通知类型调整文字颜色 */
.notification-banner.success .notification-message {
  color: #2e7d32;
}

.notification-banner.error .notification-message {
  color: #c62828;
}

.notification-banner.warning .notification-message {
  color: #ef6c00;
}

.notification-banner.info .notification-message {
  color: #1565c0;
}

.notification-close {
  flex-shrink: 0;
  background: none;
  border: none;
  padding: 4px;
  cursor: pointer;
  border-radius: 4px;
  color: #666;
  transition: all 0.2s ease;
  display: flex;
  align-items: center;
  justify-content: center;
}

.notification-close:hover {
  background: rgba(0, 0, 0, 0.05);
  color: #333;
}

/* 动画效果 */
.notification-enter-active {
  transition: all 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275);
}

.notification-leave-active {
  transition: all 0.3s ease-in;
}

.notification-enter-from {
  transform: translateX(100%);
  opacity: 0;
}

.notification-leave-to {
  transform: translateX(100%);
  opacity: 0;
}

.notification-move {
  transition: transform 0.3s ease;
}

/* 响应式设计 */
@media (max-width: 768px) {
  .notification-container {
    top: 10px;
    right: 10px;
    left: 10px;
    max-width: none;
  }
  
  .notification-content {
    padding: 12px 16px;
  }
  
  .notification-message {
    font-size: 13px;
  }
}

/* 深色模式支持 */
:global(body.dark-mode) .notification-banner {
  background: #3a3a3a !important;
  border-color: rgba(255, 255, 255, 0.2) !important;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4) !important;
}

:global(body.dark-mode) .notification-banner.success {
  background: linear-gradient(135deg, rgba(76, 175, 80, 0.2), rgba(76, 175, 80, 0.1)) !important;
  border-left-color: #4caf50 !important;
}

:global(body.dark-mode) .notification-banner.error {
  background: linear-gradient(135deg, rgba(244, 67, 54, 0.2), rgba(244, 67, 54, 0.1)) !important;
  border-left-color: #f44336 !important;
}

:global(body.dark-mode) .notification-banner.warning {
  background: linear-gradient(135deg, rgba(255, 152, 0, 0.2), rgba(255, 152, 0, 0.1)) !important;
  border-left-color: #ff9800 !important;
}

:global(body.dark-mode) .notification-banner.info {
  background: linear-gradient(135deg, rgba(33, 150, 243, 0.2), rgba(33, 150, 243, 0.1)) !important;
  border-left-color: #2196f3 !important;
}

:global(body.dark-mode) .notification-banner.success .notification-message {
  color: #66bb6a !important;
}

:global(body.dark-mode) .notification-banner.error .notification-message {
  color: #ef5350 !important;
}

:global(body.dark-mode) .notification-banner.warning .notification-message {
  color: #ffa726 !important;
}

:global(body.dark-mode) .notification-banner.info .notification-message {
  color: #42a5f5 !important;
}

:global(body.dark-mode) .notification-close {
  color: #b0b0b0 !important;
}

:global(body.dark-mode) .notification-close:hover {
  background: rgba(255, 255, 255, 0.1) !important;
  color: #e0e0e0 !important;
}
</style> 