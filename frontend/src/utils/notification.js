// 全局通知工具函数

/**
 * 显示通知
 * @param {string} message - 通知消息
 * @param {string} type - 通知类型: 'success', 'error', 'warning', 'info'
 * @param {number} duration - 显示时长(毫秒)，0表示不自动关闭
 */
export function showNotification(message, type = 'info', duration = 3000) {
  // 触发全局通知事件
  const event = new CustomEvent('global-notification', {
    detail: {
      message,
      type,
      duration
    }
  })
  window.dispatchEvent(event)
}

// 便捷方法
export const notification = {
  success: (message, duration = 3000) => showNotification(message, 'success', duration),
  error: (message, duration = 4000) => showNotification(message, 'error', duration),
  warning: (message, duration = 3500) => showNotification(message, 'warning', duration),
  info: (message, duration = 3000) => showNotification(message, 'info', duration)
}

// 默认导出
export default notification 