<template>
  <div class="login-container" :class="{ 'dark-mode': isDarkMode }">
    <!-- 背景装饰 -->
    <div class="background-decoration">
      <div class="decoration-circle circle-1"></div>
      <div class="decoration-circle circle-2"></div>
      <div class="decoration-circle circle-3"></div>
    </div>
    
    <div class="login-card">
      <!-- 顶部logo和标题区域 -->
      <div class="login-header">
        <div class="logo-container">
          <img v-if="siteInfo.site_icon" class="site-logo" :src="siteInfo.site_icon" alt="logo" @error="onLogoError" />
          <img v-else class="site-logo" src="/favicon.ico" alt="logo" @error="onLogoError" />
        </div>
        <h1 class="login-title">欢迎回来</h1>
        <p class="login-subtitle">登录到 {{ siteInfo.site_title }}</p>
      </div>

      <!-- 登录表单 -->
      <el-form :model="form" @submit.prevent="onLogin" class="login-form">
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
            class="login-button" 
            @click="onLogin" 
            :loading="loading"
          >
            <span v-if="!loading">登录</span>
            <span v-else>登录中...</span>
          </el-button>
        </el-form-item>

        <el-form-item class="form-item">
          <el-button 
            plain
            size="large"
            class="guest-button" 
            @click="onGuestLogin" 
            :loading="guestLoading"
          >
            <span v-if="!guestLoading">游客访问</span>
            <span v-else>正在进入...</span>
          </el-button>
        </el-form-item>
        
        <div v-if="siteInfo.allow_registration" class="form-footer">
          <span class="footer-text">还没有账号？</span>
          <span class="register-link" @click="$emit('show-register')">立即注册</span>
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

const emit = defineEmits(['login-success', 'show-register'])
const router = useRouter()
const form = ref({ username: '', password: '' })
const loading = ref(false)
const guestLoading = ref(false)
const siteInfo = ref({
  site_title: 'YaoList',
  allow_registration: true,
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
    
    console.log('Login 加载站点信息:', {
      background_image_url: siteInfo.value.background_image_url,
      enable_glass_effect: siteInfo.value.enable_glass_effect,
      glass_opacity: siteInfo.value.glass_opacity,
      glass_blur: siteInfo.value.glass_blur
    });
    
    // 应用主题色
    if (siteInfo.value.theme_color) {
      document.documentElement.style.setProperty('--theme-color', siteInfo.value.theme_color)
      document.documentElement.style.setProperty('--el-color-primary', siteInfo.value.theme_color)
    }
    
    // 更新页面标题
    document.title = `登录 - ${siteInfo.value.site_title}`
    
    // 应用背景图片和毛玻璃效果
    applyBackgroundAndGlassEffect()
  } catch (error) {
    console.error('加载站点信息失败:', error)
  }
}

// 应用背景图片和毛玻璃效果
function applyBackgroundAndGlassEffect() {
  const body = document.body;
  
  // 应用背景图片
  if (siteInfo.value.background_image_url && siteInfo.value.background_image_url.trim()) {
    body.style.backgroundImage = `url(${siteInfo.value.background_image_url})`;
    body.style.backgroundSize = 'cover';
    body.style.backgroundPosition = 'center';
    body.style.backgroundRepeat = 'no-repeat';
    body.style.backgroundAttachment = 'fixed';
    console.log('✅ Login 应用背景图片:', siteInfo.value.background_image_url);
  } else {
    body.style.backgroundImage = '';
    console.log('❌ Login 清除背景图片');
  }
  
  // 应用毛玻璃效果
  const glassElements = document.querySelectorAll('.login-card');
  console.log('Login 找到元素数量:', glassElements.length);
  
  glassElements.forEach(element => {
    if (siteInfo.value.enable_glass_effect && siteInfo.value.background_image_url && siteInfo.value.background_image_url.trim()) {
      // 确保数值类型正确
      const opacity = parseFloat(siteInfo.value.glass_opacity) || 0.7;
      const blur = parseFloat(siteInfo.value.glass_blur) || 10;
      
      element.style.background = `rgba(255, 255, 255, ${opacity}) !important`;
      element.style.backdropFilter = `blur(${blur}px) !important`;
      element.style.webkitBackdropFilter = `blur(${blur}px) !important`;
      element.style.border = '1px solid rgba(255, 255, 255, 0.3) !important';
      element.style.boxShadow = '0 8px 32px rgba(0, 0, 0, 0.1) !important';
      element.classList.add('glass-effect');
      console.log('✅ Login 应用毛玻璃效果到元素:', element.className, { opacity, blur });
    } else {
      element.style.background = '';
      element.style.backdropFilter = '';
      element.style.webkitBackdropFilter = '';
      element.style.border = '';
      element.style.boxShadow = '';
      element.classList.remove('glass-effect');
      console.log('❌ Login 清除毛玻璃效果:', element.className);
    }
  });
}

async function onLogin() {
  if (!form.value.username.trim()) {
    notification.error('请输入用户名')
    return
  }
  if (!form.value.password.trim()) {
    notification.error('请输入密码')
    return
  }

  loading.value = true
  try {
    const res = await axios.post('/api/login', form.value)
    if (res.status === 200 && res.data.username) {
      localStorage.setItem('yaolist_user', JSON.stringify(res.data))
      notification.success('登录成功，正在跳转...')
      emit('login-success', res.data)
      setTimeout(() => {
        router.push('/')
      }, 1000)
    } else {
      notification.error(res.data || '登录失败')
    }
  } catch (e) {
    notification.error(e.response?.data || '登录失败')
  } finally {
    loading.value = false
  }
}

async function onGuestLogin() {
  guestLoading.value = true
  try {
    const res = await axios.get('/api/guest-login')
    if (res.status === 200 && res.data.username) {
      localStorage.setItem('yaolist_user', JSON.stringify(res.data))
      notification.success('游客登录成功，正在跳转...')
      emit('login-success', res.data)
      setTimeout(() => {
        router.push('/')
      }, 1000)
    } else {
      notification.error(res.data || '游客登录失败')
    }
  } catch (e) {
    notification.error(e.response?.data || '游客登录失败')
  } finally {
    guestLoading.value = false
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
.login-container {
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

/* 登录卡片 */
.login-card {
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
.login-header {
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

.login-title {
  font-size: 2.5rem;
  font-weight: 700;
  color: #2c3e50;
  margin: 0 0 8px 0;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.login-subtitle {
  font-size: 1.1rem;
  color: #64748b;
  margin: 0;
  font-weight: 400;
}

/* 表单样式 */
.login-form {
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

/* 登录按钮 */
.login-button {
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

.login-button::before {
  content: '';
  position: absolute;
  top: 0;
  left: -100%;
  width: 100%;
  height: 100%;
  background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.2), transparent);
  transition: left 0.5s ease;
}

.login-button:hover::before {
  left: 100%;
}

.login-button:hover {
  transform: translateY(-2px);
  box-shadow: 0 12px 32px rgba(102, 126, 234, 0.4);
}

.login-button:active {
  transform: translateY(0);
}

/* 游客按钮 */
.guest-button {
  width: 100% !important;
  height: 56px !important;
  border-radius: 16px !important;
  font-size: 16px !important;
  font-weight: 600 !important;
  background: rgba(108, 117, 125, 0.1) !important;
  border: 2px solid #6c757d !important;
  color: #6c757d !important;
  transition: all 0.3s ease !important;
  position: relative !important;
  overflow: hidden !important;
  box-sizing: border-box !important;
  display: block !important;
  margin-top: 16px !important;
}

.guest-button:deep(.el-button) {
  background: rgba(108, 117, 125, 0.1) !important;
  border: 2px solid #6c757d !important;
  color: #6c757d !important;
}

.guest-button:hover {
  background: #6c757d !important;
  color: white !important;
  border-color: #6c757d !important;
  transform: translateY(-2px) !important;
  box-shadow: 0 8px 25px rgba(108, 117, 125, 0.3) !important;
}

.guest-button:hover:deep(.el-button) {
  background: #6c757d !important;
  color: white !important;
  border-color: #6c757d !important;
}

.guest-button:active {
  transform: translateY(1px) !important;
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

.register-link {
  color: var(--theme-color, #667eea);
  cursor: pointer;
  font-size: 14px;
  font-weight: 600;
  text-decoration: none;
  transition: all 0.3s ease;
  position: relative;
}

.register-link::after {
  content: '';
  position: absolute;
  bottom: -2px;
  left: 0;
  width: 0;
  height: 2px;
  background: var(--theme-color, #667eea);
  transition: width 0.3s ease;
}

.register-link:hover::after {
  width: 100%;
}

.register-link:hover {
  color: #764ba2;
}

/* 深色模式样式 */
.dark-mode {
  background: #1a1a2e !important;
}

.dark-mode .login-card {
  background: rgba(45, 45, 45, 0.95) !important;
  border-color: rgba(255, 255, 255, 0.1) !important;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3) !important;
}

.dark-mode .login-title {
  background: linear-gradient(135deg, #66b1ff 0%, #a855f7 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.dark-mode .login-subtitle {
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

.dark-mode .login-button {
  background: linear-gradient(135deg, #66b1ff 0%, #a855f7 100%) !important;
  box-shadow: 0 8px 24px rgba(102, 177, 255, 0.3) !important;
}

.dark-mode .login-button:hover {
  box-shadow: 0 12px 32px rgba(102, 177, 255, 0.4) !important;
}

.dark-mode .form-footer {
  border-top-color: #4a5568 !important;
}

.dark-mode .register-link {
  color: #66b1ff !important;
}

.dark-mode .register-link:hover {
  color: #a855f7 !important;
}

.dark-mode .register-link::after {
  background: #66b1ff !important;
}

.dark-mode .guest-button {
  background: rgba(148, 163, 184, 0.1) !important;
  border-color: #94a3b8 !important;
  color: #94a3b8 !important;
}

.dark-mode .guest-button:hover {
  background: #94a3b8 !important;
  color: #1a1a2e !important;
  border-color: #94a3b8 !important;
  box-shadow: 0 8px 25px rgba(148, 163, 184, 0.3) !important;
}

/* 响应式设计 */
@media (max-width: 480px) {
  .login-card {
    width: 90%;
    padding: 32px 24px;
    margin: 20px;
  }
  
  .login-title {
    font-size: 2rem;
  }
  
  .site-logo {
    width: 60px;
    height: 60px;
  }
}
</style> 