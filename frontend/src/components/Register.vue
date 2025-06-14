<template>
  <div class="register-container" :class="{ 'dark-mode': isDarkMode }">
    <!-- 背景装饰 -->
    <div class="background-decoration">
      <div class="decoration-circle circle-1"></div>
      <div class="decoration-circle circle-2"></div>
      <div class="decoration-circle circle-3"></div>
    </div>
    
    <div class="register-card">
      <!-- 顶部logo和标题区域 -->
      <div class="register-header">
        <div class="logo-container">
          <img v-if="siteInfo.site_icon" class="site-logo" :src="siteInfo.site_icon" alt="logo" @error="onLogoError" />
          <img v-else class="site-logo" src="/favicon.ico" alt="logo" @error="onLogoError" />
        </div>
        <h1 class="register-title">加入我们</h1>
        <p class="register-subtitle">注册 {{ siteInfo.site_title }} 账号</p>
      </div>

      <!-- 注册表单 -->
      <el-form :model="form" @submit.prevent="onRegister" class="register-form">
        <el-form-item class="form-item">
          <div class="input-wrapper">
            <div class="input-icon">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/>
                <circle cx="12" cy="7" r="4"/>
              </svg>
            </div>
            <el-input 
              v-model="form.username" 
              placeholder="请输入用户名" 
              size="large"
              class="custom-input"
            />
          </div>
        </el-form-item>
        
        <el-form-item class="form-item">
          <div class="input-wrapper">
            <div class="input-icon">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
                <circle cx="12" cy="16" r="1"/>
                <path d="M7 11V7a5 5 0 0110 0v4"/>
              </svg>
            </div>
            <el-input 
              v-model="form.password" 
              type="password" 
              placeholder="请输入密码" 
              size="large"
              class="custom-input"
              show-password
            />
          </div>
        </el-form-item>
        
        <el-form-item class="form-item">
          <el-button 
            type="primary" 
            size="large"
            class="register-button" 
            @click="onRegister" 
            :loading="loading"
          >
            <span v-if="!loading">注册</span>
            <span v-else>注册中...</span>
          </el-button>
        </el-form-item>
        
        <div class="form-footer">
          <span class="footer-text">已有账号？</span>
          <span class="login-link" @click="$emit('show-login')">立即登录</span>
        </div>
      </el-form>
    </div>
  </div>
</template>
<script setup>
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import axios from 'axios'
import notification from '../utils/notification.js'

const emit = defineEmits(['register-success', 'show-login'])
const router = useRouter()
const form = ref({ username: '', password: '' })
const loading = ref(false)
const siteInfo = ref({
  site_title: 'YaoList',
  site_icon: '',
  favicon: 'https://api.ylist.org/logo/logo.svg'
})
const isDarkMode = ref(localStorage.getItem('yaolist_dark_mode') === 'true')

function onLogoError(e) {
  e.target.style.display = 'none'
}

// 加载站点信息
async function loadSiteInfo() {
  try {
    const res = await axios.get('/api/site-info')
    siteInfo.value = res.data
    
    // 应用主题色
    if (siteInfo.value.theme_color) {
      document.documentElement.style.setProperty('--theme-color', siteInfo.value.theme_color)
      document.documentElement.style.setProperty('--el-color-primary', siteInfo.value.theme_color)
    }
    
    // 更新页面标题
    document.title = `注册 - ${siteInfo.value.site_title}`
  } catch (error) {
    console.error('加载站点信息失败:', error)
  }
}

async function onRegister() {
  if (!form.value.username.trim()) {
    notification.error('请输入用户名')
    return
  }
  if (!form.value.password.trim()) {
    notification.error('请输入密码')
    return
  }
  if (form.value.password.length < 6) {
    notification.error('密码长度至少6位')
    return
  }

  loading.value = true
  try {
    const res = await axios.post('/api/register', form.value)
    if (res.status === 200 && res.data.username) {
      localStorage.setItem('yaolist_user', JSON.stringify(res.data))
      notification.success('注册成功，正在跳转...')
      emit('register-success', res.data)
      setTimeout(() => {
        router.push('/')
      }, 1000)
    } else {
      notification.error(res.data || '注册失败')
    }
  } catch (e) {
    notification.error(e.response?.data || '注册失败')
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  loadSiteInfo()
  
  // 应用保存的主题设置
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode')
  }
})
</script>
<style scoped>
.register-container {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #667eea;
  position: relative;
  overflow: hidden;
}

/* 背景装饰 */
.background-decoration {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 1;
}

.decoration-circle {
  position: absolute;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.1);
  animation: float 6s ease-in-out infinite;
}

.circle-1 {
  width: 200px;
  height: 200px;
  top: 10%;
  left: 10%;
  animation-delay: 0s;
}

.circle-2 {
  width: 150px;
  height: 150px;
  top: 60%;
  right: 15%;
  animation-delay: 2s;
}

.circle-3 {
  width: 100px;
  height: 100px;
  bottom: 20%;
  left: 20%;
  animation-delay: 4s;
}

@keyframes float {
  0%, 100% {
    transform: translateY(0px) rotate(0deg);
    opacity: 0.7;
  }
  50% {
    transform: translateY(-20px) rotate(180deg);
    opacity: 1;
  }
}

/* 注册卡片 */
.register-card {
  width: 420px;
  background: rgba(255, 255, 255, 0.95);
  backdrop-filter: blur(20px);
  border-radius: 24px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.1);
  padding: 48px 40px;
  position: relative;
  z-index: 2;
  border: 1px solid rgba(255, 255, 255, 0.2);
  animation: slideUp 0.6s ease-out;
}

@keyframes slideUp {
  from {
    opacity: 0;
    transform: translateY(30px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* 头部区域 */
.register-header {
  text-align: center;
  margin-bottom: 40px;
}

.logo-container {
  margin-bottom: 24px;
}

.site-logo {
  width: 80px;
  height: 80px;
  border-radius: 20px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
  transition: transform 0.3s ease;
}

.site-logo:hover {
  transform: scale(1.05);
}

.register-title {
  font-size: 2.5rem;
  font-weight: 700;
  color: #2c3e50;
  margin: 0 0 8px 0;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.register-subtitle {
  font-size: 1.1rem;
  color: #64748b;
  margin: 0;
  font-weight: 400;
}

/* 表单样式 */
.register-form {
  width: 100%;
}

.form-item {
  margin-bottom: 24px;
  width: 100%;
}

.form-item :deep(.el-form-item__content) {
  width: 100% !important;
}

.input-wrapper {
  position: relative;
  display: block;
  width: 100%;
}

.input-icon {
  position: absolute;
  left: 16px;
  top: 50%;
  transform: translateY(-50%);
  z-index: 3;
  color: #94a3b8;
  transition: color 0.3s ease;
}

.custom-input {
  width: 100%;
  display: block;
}

.custom-input :deep(.el-input__wrapper) {
  padding-left: 48px;
  height: 56px;
  border-radius: 16px;
  border: 2px solid #e2e8f0;
  background: rgba(255, 255, 255, 0.8);
  backdrop-filter: blur(10px);
  transition: all 0.3s ease;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.05);
  width: 100% !important;
  box-sizing: border-box !important;
}

.custom-input :deep(.el-input__inner) {
  font-size: 16px;
  color: #2c3e50;
  font-weight: 500;
}

.custom-input :deep(.el-input__wrapper:hover) {
  border-color: var(--theme-color, #667eea);
  box-shadow: 0 4px 20px rgba(102, 126, 234, 0.15);
}

.custom-input :deep(.el-input__wrapper.is-focus) {
  border-color: var(--theme-color, #667eea);
  box-shadow: 0 0 0 4px rgba(102, 126, 234, 0.1);
}

.input-wrapper:focus-within .input-icon {
  color: var(--theme-color, #667eea);
}

/* 注册按钮 */
.register-button {
  width: 100% !important;
  height: 56px;
  border-radius: 16px;
  font-size: 16px;
  font-weight: 600;
  background: linear-gradient(135deg, var(--theme-color, #667eea) 0%, #764ba2 100%);
  border: none;
  box-shadow: 0 8px 24px rgba(102, 126, 234, 0.3);
  transition: all 0.3s ease;
  position: relative;
  overflow: hidden;
  box-sizing: border-box !important;
  display: block;
}

.register-button::before {
  content: '';
  position: absolute;
  top: 0;
  left: -100%;
  width: 100%;
  height: 100%;
  background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.2), transparent);
  transition: left 0.5s ease;
}

.register-button:hover::before {
  left: 100%;
}

.register-button:hover {
  transform: translateY(-2px);
  box-shadow: 0 12px 32px rgba(102, 126, 234, 0.4);
}

.register-button:active {
  transform: translateY(0);
}

/* 底部链接 */
.form-footer {
  text-align: center;
  margin-top: 32px;
  padding-top: 24px;
  border-top: 1px solid #e2e8f0;
}

.footer-text {
  color: #64748b;
  font-size: 14px;
  margin-right: 8px;
}

.login-link {
  color: var(--theme-color, #667eea);
  cursor: pointer;
  font-size: 14px;
  font-weight: 600;
  text-decoration: none;
  transition: all 0.3s ease;
  position: relative;
}

.login-link::after {
  content: '';
  position: absolute;
  bottom: -2px;
  left: 0;
  width: 0;
  height: 2px;
  background: var(--theme-color, #667eea);
  transition: width 0.3s ease;
}

.login-link:hover::after {
  width: 100%;
}

.login-link:hover {
  color: #764ba2;
}

/* 深色模式样式 */
.dark-mode {
  background: #1a1a2e !important;
}

.dark-mode .register-card {
  background: rgba(45, 45, 45, 0.95) !important;
  border-color: rgba(255, 255, 255, 0.1) !important;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3) !important;
}

.dark-mode .register-title {
  background: linear-gradient(135deg, #66b1ff 0%, #a855f7 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.dark-mode .register-subtitle {
  color: #94a3b8 !important;
}

.dark-mode .footer-text {
  color: #94a3b8 !important;
}

.dark-mode .decoration-circle {
  background: rgba(102, 177, 255, 0.1) !important;
}

.dark-mode .custom-input :deep(.el-input__wrapper) {
  background: rgba(58, 58, 58, 0.8) !important;
  border-color: #4a5568 !important;
}

.dark-mode .custom-input :deep(.el-input__inner) {
  color: #e2e8f0 !important;
}

.dark-mode .custom-input :deep(.el-input__wrapper:hover) {
  border-color: #66b1ff !important;
  box-shadow: 0 4px 20px rgba(102, 177, 255, 0.15) !important;
}

.dark-mode .custom-input :deep(.el-input__wrapper.is-focus) {
  border-color: #66b1ff !important;
  box-shadow: 0 0 0 4px rgba(102, 177, 255, 0.1) !important;
}

.dark-mode .input-icon {
  color: #94a3b8 !important;
}

.dark-mode .input-wrapper:focus-within .input-icon {
  color: #66b1ff !important;
}

.dark-mode .register-button {
  background: linear-gradient(135deg, #66b1ff 0%, #a855f7 100%) !important;
  box-shadow: 0 8px 24px rgba(102, 177, 255, 0.3) !important;
}

.dark-mode .register-button:hover {
  box-shadow: 0 12px 32px rgba(102, 177, 255, 0.4) !important;
}

.dark-mode .form-footer {
  border-top-color: #4a5568 !important;
}

.dark-mode .login-link {
  color: #66b1ff !important;
}

.dark-mode .login-link:hover {
  color: #a855f7 !important;
}

.dark-mode .login-link::after {
  background: #66b1ff !important;
}

/* 响应式设计 */
@media (max-width: 480px) {
  .register-card {
    width: 90%;
    padding: 32px 24px;
    margin: 20px;
  }
  
  .register-title {
    font-size: 2rem;
  }
  
  .site-logo {
    width: 60px;
    height: 60px;
  }
}
</style> 