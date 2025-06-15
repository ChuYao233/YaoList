<template>
  <div class="yaolist-flex-root" :class="{ 'dark-mode': isDarkMode }" :style="backgroundStyle">
    <!-- 主题切换按钮 -->
    <div class="theme-toggle-container">
      <button class="theme-toggle-btn" @click="toggleDarkMode" :title="isDarkMode ? '切换到浅色模式' : '切换到深色模式'">
        <svg v-if="isDarkMode" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="5"/>
          <line x1="12" y1="1" x2="12" y2="3"/>
          <line x1="12" y1="21" x2="12" y2="23"/>
          <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/>
          <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/>
          <line x1="1" y1="12" x2="3" y2="12"/>
          <line x1="21" y1="12" x2="23" y2="12"/>
          <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/>
          <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
        </svg>
        <svg v-else width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/>
        </svg>
      </button>
    </div>
    
    <!-- 中间内容区域 -->
    <div class="yaolist-flex-center">
      <!-- 站点标题 -->
      <div class="yaolist-title title-above-card">
        <div class="title-left">
          <img v-if="siteInfo.site_icon" class="yaolist-logo-large" :src="siteInfo.site_icon" alt="logo" @error="onLogoError" />
          <img v-else class="yaolist-logo-large" src="/favicon.ico" alt="logo" @error="onLogoError" />
          <span class="title-text">{{ siteInfo.site_title }}</span>
        </div>
      </div>
      <div class="yaolist-card">
        <!-- 路径面包屑 -->
        <div class="yaolist-path-breadcrumb">
          <template v-for="(crumb, idx) in pathBreadcrumbs" :key="crumb.path">
            <span
              class="yaolist-breadcrumb clickable"
              @click="navigateTo(crumb.path)"
            >
              {{ crumb.name }}
            </span>
            <span v-if="idx !== pathBreadcrumbs.length - 1" class="yaolist-breadcrumb-sep">/</span>
          </template>
        </div>
        
        <!-- 文件信息和操作栏 -->
        <div class="file-info-header">
          <div class="file-info-left">
            <div class="file-icon">
              <span 
                class="file-icon-svg" 
                :style="{ color: getFileIconColor() }"
                v-html="getFileIconSvg()"
              ></span>
            </div>
            <div class="file-details">
              <div class="file-name">{{ fileName }}</div>
              <div class="file-meta">
                <span v-if="fileSize" class="file-size">{{ formatFileSize(fileSize) }}</span>
                <span v-if="fileModified" class="file-modified">{{ formatDate(fileModified) }}</span>
              </div>
            </div>
          </div>
          <div class="file-actions">
            <button class="action-btn download" @click="downloadFile">
              <svg width="20" height="20" viewBox="0 0 24 24">
                <path fill="currentColor" d="M5 20h14v-2H5v2zm7-18c-.55 0-1 .45-1 1v8.59l-3.29-3.3a.996.996 0 1 0-1.41 1.41l5 5c.39.39 1.02.39 1.41 0l5-5a.996.996 0 1 0-1.41-1.41L13 11.59V3c0-.55-.45-1-1-1z"/>
              </svg>
              下载
            </button>
            <button class="action-btn copy" @click="copyLink">
              <svg width="20" height="20" viewBox="0 0 24 24">
                <path fill="currentColor" d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
              </svg>
              复制链接
            </button>
          </div>
        </div>

        <!-- 预览内容区域 -->
        <div class="file-preview-content">
          <div v-if="loading" class="loading-container">
            <div class="loading-spinner"></div>
            <div class="loading-text">加载中...</div>
          </div>

          <!-- 图片预览 -->
          <div v-else-if="previewType === 'image'" class="preview-image">
            <img :src="fileUrl" @load="onImageLoad" @error="onPreviewError" />
          </div>

          <!-- 视频预览 -->
          <div v-else-if="previewType === 'video'" class="preview-video">
            <video 
              :src="fileUrl" 
              controls 
              :autoplay="siteInfo.preview_auto_play_video" 
              @error="onPreviewError"
              ref="videoPlayer"
              class="video-player"
            >
              您的浏览器不支持视频播放
            </video>
          </div>

          <!-- 音频预览 -->
          <div v-else-if="previewType === 'audio'" class="preview-audio">
            <div class="audio-cover">
              <img :src="siteInfo.preview_audio_cover || '/favicon.ico'" alt="音频封面" @error="handleAudioCoverError" />
            </div>
            <div class="audio-info">
              <div class="audio-title">{{ fileName }}</div>
              <audio 
                :src="fileUrl" 
                controls 
                :autoplay="siteInfo.preview_auto_play_audio" 
                @error="onPreviewError"
              >
                您的浏览器不支持音频播放
              </audio>
            </div>
          </div>

          <!-- 文本预览 -->
          <div v-else-if="previewType === 'text'" class="preview-text">
            <div class="text-toolbar">
              <select v-model="textLanguage" class="language-select">
                <option value="">自动检测</option>
                <option value="javascript">JavaScript</option>
                <option value="python">Python</option>
                <option value="java">Java</option>
                <option value="html">HTML</option>
                <option value="css">CSS</option>
                <option value="json">JSON</option>
                <option value="xml">XML</option>
                <option value="sql">SQL</option>
                <option value="markdown">Markdown</option>
                <option value="yaml">YAML</option>
                <option value="go">Go</option>
                <option value="rust">Rust</option>
                <option value="cpp">C++</option>
                <option value="c">C</option>
                <option value="php">PHP</option>
                <option value="vue">Vue</option>
                <option value="typescript">TypeScript</option>
                <option value="shell">Shell</option>
                <option value="batch">Batch</option>
              </select>
            </div>
            <div v-if="isMarkdown" class="markdown-content" v-html="renderedContent"></div>
            <pre v-else class="code-content" :class="textLanguage"><code v-html="highlightCode(textContent, textLanguage)"></code></pre>
          </div>

          <!-- M3U8 流媒体预览 -->
          <div v-else-if="previewType === 'm3u8'" class="preview-m3u8">
            <video 
              ref="hlsPlayer" 
              controls 
              :autoplay="siteInfo.preview_auto_play_video" 
              @error="onPreviewError"
              class="video-player"
            >
              您的浏览器不支持HLS流媒体播放
            </video>
          </div>

          <!-- Office文档预览 -->
          <div v-else-if="previewType === 'office'" class="preview-office">
            <div class="office-toolbar">
              <select v-model="selectedOfficeViewer" @change="switchOfficeViewer" class="viewer-select">
                <option v-for="(url, name) in officeViewers" :key="name" :value="name">{{ name }}</option>
              </select>
            </div>
            <iframe :src="viewerUrl" frameborder="0" @error="onPreviewError"></iframe>
          </div>

          <!-- PDF预览 -->
          <div v-else-if="previewType === 'pdf'" class="preview-pdf">
            <iframe :src="viewerUrl" frameborder="0" @error="onPreviewError"></iframe>
          </div>

          <!-- EPUB预览 -->
          <div v-else-if="previewType === 'epub'" class="preview-epub">
            <iframe :src="viewerUrl" frameborder="0" @error="onPreviewError"></iframe>
          </div>

          <!-- 外部预览 -->
          <div v-else-if="previewType === 'external'" class="preview-external">
            <div class="external-toolbar">
              <select v-model="selectedExternalViewer" @change="switchExternalViewer" class="viewer-select">
                <option v-for="(url, name) in externalViewers" :key="name" :value="name">{{ name }}</option>
              </select>
            </div>
            <iframe :src="externalViewerUrl" frameborder="0" @error="onPreviewError"></iframe>
          </div>

          <!-- 错误信息 -->
          <div v-else-if="error" class="preview-error">
            <div class="error-icon">⚠️</div>
            <div class="error-message">{{ error }}</div>
            <button @click="downloadFile" class="download-btn">下载文件</button>
          </div>

          <!-- 不支持的类型 -->
          <div v-else class="preview-unsupported">
            <div class="unsupported-icon">📄</div>
            <div class="unsupported-message">暂不支持预览此文件类型</div>
            <button @click="downloadFile" class="download-btn">下载文件</button>
          </div>
        </div>
      </div>
    </div>
    
    <!-- 底部登录信息 -->
    <div class="yaolist-bottom-userinfo userinfo-float">
      <template v-if="user && user.username">
        <span style="font-weight: bold; color: #333; cursor:pointer;" @click="router.push('/admin')">{{ user.username }}</span>
        <span style="margin: 0 8px;">|</span>
        <span class="userinfo-action" @click="handleLogout" style="cursor:pointer; color:#409EFF;">登出</span>
      </template>
      <template v-else>
        <span class="userinfo-action" @click="handleLogin" style="cursor:pointer; color:#409EFF;">登录</span>
        <template v-if="siteInfo.allow_registration">
          <span style="margin: 0 8px;">|</span>
          <span class="userinfo-action" @click="handleRegister" style="cursor:pointer; color:#409EFF;">注册</span>
        </template>
      </template>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, watch } from 'vue';
import { useRouter, useRoute } from 'vue-router';
// 移除Element Plus消息组件，使用自定义消息提示
// 移除Element Plus图标导入，使用自定义扁平化SVG图标
import axios from 'axios';
import notification from './utils/notification.js';

const router = useRouter();
const route = useRoute();

// 响应式数据
const loading = ref(true);
const error = ref('');
const fileName = ref('');
const fileSize = ref(0);
const fileModified = ref('');
const fileUrl = ref('');
const previewType = ref('');
const textContent = ref('');
const isMarkdown = ref(false);
const renderedContent = ref('');
const textLanguage = ref('');
const selectedOfficeViewer = ref('');
const officeViewers = ref({});
const viewerUrl = ref('');
const selectedExternalViewer = ref('');
const externalViewers = ref({});
const externalViewerUrl = ref('');
const user = ref({});
const isDarkMode = ref(localStorage.getItem('yaolist_dark_mode') === 'true');

// 站点信息
const siteInfo = ref({
  site_title: 'YaoList',
  site_description: '现代化的文件管理系统',
  theme_color: '#1976d2',
  site_icon: '',
      favicon: 'https://api.ylist.org/logo/logo.svg',
  allow_registration: true,
  items_per_page: 20,
  preview_text_types: 'txt,htm,html,xml,java,properties,sql,js,md,json,conf,ini,vue,php,py,bat,gitignore,yml,go,sh,c,cpp,h,hpp,tsx,vtt,srt,ass,rs,lrc',
  preview_audio_types: 'mp3,flac,ogg,m4a,wav,opus,wma',
  preview_video_types: 'mp4,mkv,avi,mov,rmvb,webm,flv',
  preview_image_types: 'jpg,tiff,jpeg,png,gif,bmp,svg,ico,swf,webp',
  preview_proxy_types: 'm3u8',
  preview_proxy_ignore_headers: 'authorization,referer',
  preview_external: '{}',
  preview_iframe: '{"doc,docx,xls,xlsx,ppt,pptx":{"Microsoft":"https://view.officeapps.live.com/op/view.aspx?src=$e_url","Google":"https://docs.google.com/gview?url=$e_url&embedded=true"},"pdf":{"PDF.js":"https://alist-org.github.io/pdf.js/web/viewer.html?file=$e_url"},"epub":{"EPUB.js":"https://alist-org.github.io/static/epub.js/viewer.html?url=$e_url"}}',
  preview_audio_cover: 'https://jsd.nn.ci/gh/alist-org/logo@main/logo.svg',
  preview_auto_play_audio: false,
  preview_auto_play_video: false,
  preview_default_archive: false,
  preview_readme_render: true,
  preview_readme_filter_script: true
});

// refs
const videoPlayer = ref(null);
const hlsPlayer = ref(null);

// 初始化用户信息
try {
  user.value = JSON.parse(localStorage.getItem('yaolist_user') || '{}')
} catch {
  user.value = {}
}

// 计算属性
const filePath = computed(() => {
  // 清理路径：移除双斜杠，确保路径格式正确
  let path = decodeURIComponent(route.path);
  // 移除末尾的斜杠（如果有）
  path = path.replace(/\/+$/, '');
  // 替换多个连续斜杠为单个斜杠
  path = path.replace(/\/+/g, '/');
  // 确保路径以/开头
  if (!path.startsWith('/')) {
    path = '/' + path;
  }
  return path;
});

// 路径面包屑
const pathBreadcrumbs = computed(() => {
  const parts = filePath.value.replace(/\\/g, '/').split('/').filter(Boolean);
  const crumbs = [{ name: '🏠主页', path: '/' }];
  let path = '';
  for (const part of parts) {
    path += '/' + part;
    crumbs.push({ name: decodeURIComponent(part), path });
  }
  return crumbs;
});

const backgroundStyle = computed(() => {
  if (!siteInfo.value.background_url) return {};
  return {
    backgroundImage: `url(${siteInfo.value.background_url})`,
    backgroundSize: 'cover',
    backgroundPosition: 'center',
    backgroundAttachment: 'fixed',
    minHeight: '100vh',
    transition: 'background-image 0.3s ease'
  };
});

// 方法
function formatFileSize(size) {
  if (!size) return '-';
  if (size < 1024) return size + ' B';
  if (size < 1024 * 1024) return (size / 1024).toFixed(1) + ' KB';
  if (size < 1024 * 1024 * 1024) return (size / 1024 / 1024).toFixed(1) + ' MB';
  return (size / 1024 / 1024 / 1024).toFixed(1) + ' GB';
}

function formatDate(date) {
  if (!date) return '-';
  const d = typeof date === 'string' ? new Date(date) : date;
  if (isNaN(d.getTime())) return '-';
  const pad = n => n.toString().padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function onLogoError(e) {
  e.target.style.display = 'none';
}

function navigateTo(path) {
  router.push(path);
}

function getFileIconSvg() {
  if (!previewType.value) {
    return `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
      <polyline points="14,2 14,8 20,8"/>
    </svg>`;
  }
  
  switch (previewType.value) {
    case 'video':
    case 'm3u8':
      return `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <polygon points="23 7 16 12 23 17 23 7"/>
        <rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>
      </svg>`;
    case 'audio':
      return `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M9 18V5l12-2v13"/>
        <circle cx="6" cy="18" r="3"/>
        <circle cx="18" cy="16" r="3"/>
      </svg>`;
    case 'image':
      return `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
        <circle cx="8.5" cy="8.5" r="1.5"/>
        <polyline points="21,15 16,10 5,21"/>
      </svg>`;
    default:
      return `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
        <polyline points="14,2 14,8 20,8"/>
      </svg>`;
  }
}

function getFileIconColor() {
  if (!previewType.value) return '#6b7280';
  switch (previewType.value) {
    case 'video':
    case 'm3u8':
      return '#f59e0b';
    case 'audio':
      return '#8b5cf6';
    case 'image':
      return '#10b981';
    default:
      return '#6b7280';
  }
}

function handleLogin() {
  router.push('/login');
}

function handleRegister() {
  router.push('/register');
}

function handleLogout() {
  user.value = {};
  localStorage.removeItem('yaolist_user');
  notification.success('已成功登出');
  setTimeout(() => {
    router.push('/login');
  }, 1000);
}

function getFileExtension(filename) {
  return filename.split('.').pop() || '';
}

function canPreview(filename) {
  const ext = getFileExtension(filename).toLowerCase();
  
  const textTypes = siteInfo.value.preview_text_types.split(',').map(t => t.trim());
  const audioTypes = siteInfo.value.preview_audio_types.split(',').map(t => t.trim());
  const videoTypes = siteInfo.value.preview_video_types.split(',').map(t => t.trim());
  const imageTypes = siteInfo.value.preview_image_types.split(',').map(t => t.trim());
  const proxyTypes = siteInfo.value.preview_proxy_types.split(',').map(t => t.trim());
  
  // 解析外部预览配置
  let externalConfig = {};
  try {
    externalConfig = JSON.parse(siteInfo.value.preview_external);
  } catch (e) {
    console.error('解析外部预览配置失败:', e);
  }
  
  // 检查是否在外部预览配置中
  for (const types in externalConfig) {
    if (types.split(',').map(t => t.trim()).includes(ext)) {
      return 'external';
    }
  }
  
  // 解析iframe配置
  let iframeConfig = {};
  try {
    iframeConfig = JSON.parse(siteInfo.value.preview_iframe);
  } catch (e) {
    console.error('解析iframe配置失败:', e);
  }
  
  // 检查是否在iframe配置中
  for (const types in iframeConfig) {
    if (types.split(',').map(t => t.trim()).includes(ext)) {
      return 'iframe';
    }
  }
  
  if (textTypes.includes(ext)) return 'text';
  if (audioTypes.includes(ext)) return 'audio';
  if (videoTypes.includes(ext)) return 'video';
  if (imageTypes.includes(ext)) return 'image';
  if (proxyTypes.includes(ext)) return 'proxy';
  
  return false;
}

async function loadSiteInfo() {
  try {
    const res = await axios.get('/api/site-info');
    siteInfo.value = res.data;
    
    console.log('FileDetail 加载站点信息:', {
      background_image_url: siteInfo.value.background_image_url,
      enable_glass_effect: siteInfo.value.enable_glass_effect,
      glass_opacity: siteInfo.value.glass_opacity,
      glass_blur: siteInfo.value.glass_blur
    });
    
    // 应用背景图片和毛玻璃效果
    applyBackgroundAndGlassEffect();
  } catch (error) {
    console.error('加载站点信息失败:', error);
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
    console.log('✅ FileDetail 应用背景图片:', siteInfo.value.background_image_url);
  } else {
    body.style.backgroundImage = '';
    console.log('❌ FileDetail 清除背景图片');
  }
  
  // 应用毛玻璃效果
  const glassElements = document.querySelectorAll('.yaolist-card, .file-info-header, .preview-text, .preview-audio, .preview-office, .preview-pdf, .preview-epub, .preview-error, .preview-unsupported');
  console.log('FileDetail 找到元素数量:', glassElements.length);
  
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
      console.log('✅ FileDetail 应用毛玻璃效果到元素:', element.className, { opacity, blur });
    } else {
      element.style.background = '';
      element.style.backdropFilter = '';
      element.style.webkitBackdropFilter = '';
      element.style.border = '';
      element.style.boxShadow = '';
      element.classList.remove('glass-effect');
      console.log('❌ FileDetail 清除毛玻璃效果:', element.className);
    }
  });
}

// 权限常量
const PERM_DOWNLOAD = 2;

// 检查是否拥有特定权限
function hasPermission(permValue) {
  if (!user.value || !user.value.permissions) return false;
  if (user.value.permissions === -1) return true; // 管理员拥有所有权限
  return (user.value.permissions & permValue) !== 0;
}

async function loadFileInfo() {
  try {
    loading.value = true;
    error.value = '';
    
    // 获取文件信息
    const res = await axios.get('/api/fileinfo', {
      params: { 
        path: filePath.value,
        'x-username': user.value.username || 'guest'  // 如果未登录则使用guest
      },
      headers: {
        'x-username': user.value.username || 'guest'  // 在header中也设置用户名
      }
    });
    
    const fileInfo = res.data;
    fileName.value = fileInfo.name;
    fileSize.value = fileInfo.size;
    fileModified.value = fileInfo.modified;
    fileUrl.value = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
    
    // 判断预览类型
    const type = canPreview(fileName.value);
    if (!type) {
      error.value = '不支持预览此文件类型';
      return;
    }
    
    previewType.value = type;
    
    // 根据类型加载内容
    if (type === 'text') {
      await loadTextContent();
    } else if (type === 'proxy') {
      await loadM3U8Content();
    } else if (type === 'iframe') {
      await loadIframeContent();
    } else if (type === 'external') {
      await loadExternalContent();
    }
    
  } catch (err) {
    if (err.response?.status === 401) {
      error.value = '无权访问此文件，请先登录';
      // 可以选择跳转到登录页面
      // router.push('/login');
    } else {
    error.value = err.response?.data || '加载文件失败';
    }
  } finally {
    loading.value = false;
  }
}

async function loadTextContent() {
  try {
    // 创建带认证的URL
    const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
    const res = await fetch(downloadUrl);
    if (!res.ok) {
      throw new Error('加载文本内容失败');
    }
    textContent.value = await res.text();
    
    const ext = getFileExtension(fileName.value).toLowerCase();
    if (ext === 'md' && siteInfo.value.preview_readme_render) {
      isMarkdown.value = true;
      if (window.marked) {
        let content = textContent.value;
        
        // 如果启用了脚本过滤，移除脚本标签
        if (siteInfo.value.preview_readme_filter_script) {
          content = content.replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, '');
          content = content.replace(/javascript:/gi, '');
          content = content.replace(/on\w+\s*=/gi, '');
        }
        
        renderedContent.value = window.marked.parse(content);
      }
    }
    
    // 自动检测语言
    if (!textLanguage.value) {
      textLanguage.value = detectLanguage(ext);
    }
    
    // 应用语法高亮
    setTimeout(() => {
      if (window.Prism) {
        window.Prism.highlightAll();
      }
    }, 100);
    
  } catch (err) {
    error.value = '加载文本内容失败';
  }
}

async function loadM3U8Content() {
  try {
    // 创建带认证的URL
    const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
    const res = await fetch(downloadUrl);
    if (!res.ok) {
      throw new Error('加载M3U8内容失败');
    }
    m3u8Content.value = await res.text();
    
    // 设置HLS播放器
    if (hlsPlayer.value && window.Hls && window.Hls.isSupported()) {
      const hls = new window.Hls();
      hls.loadSource(downloadUrl);
      hls.attachMedia(hlsPlayer.value);
      hls.on(window.Hls.Events.ERROR, (event, data) => {
        console.error('HLS错误:', data);
        error.value = 'HLS流媒体加载失败';
      });
    } else if (hlsPlayer.value.canPlayType('application/vnd.apple.mpegurl')) {
      // Safari原生支持
      hlsPlayer.value.src = downloadUrl;
    } else {
      error.value = '浏览器不支持HLS流媒体播放';
    }
  } catch (err) {
    error.value = err.response?.data || '加载M3U8内容失败';
  }
}

async function loadIframeContent() {
  try {
    const iframeConfig = JSON.parse(siteInfo.value.preview_iframe);
  const ext = getFileExtension(fileName.value).toLowerCase();
  
    // 查找匹配的配置
    for (const [types, viewers] of Object.entries(iframeConfig)) {
    if (types.split(',').map(t => t.trim()).includes(ext)) {
        externalViewers.value = viewers;
        if (!selectedExternalViewer.value) {
          selectedExternalViewer.value = Object.keys(viewers)[0];
        }
        const viewerUrl = viewers[selectedExternalViewer.value];
        // 创建带认证的URL
        const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
        const encodedUrl = encodeURIComponent(`${window.location.origin}${downloadUrl}`);
        iframeUrl.value = viewerUrl.replace('$e_url', encodedUrl);
      break;
    }
    }
  } catch (err) {
    error.value = '加载iframe预览失败';
  }
}

function switchOfficeViewer() {
  if (selectedOfficeViewer.value && officeViewers.value[selectedOfficeViewer.value]) {
    const template = officeViewers.value[selectedOfficeViewer.value];
    const encodedUrl = encodeURIComponent(window.location.origin + fileUrl.value);
    const rawUrl = window.location.origin + fileUrl.value;
    viewerUrl.value = template.replace('$e_url', encodedUrl).replace('$url', rawUrl);
  }
}

async function loadExternalContent() {
  try {
    const externalConfig = JSON.parse(siteInfo.value.preview_external);
  const ext = getFileExtension(fileName.value).toLowerCase();
  
    // 查找匹配的配置
    for (const [types, viewers] of Object.entries(externalConfig)) {
    if (types.split(',').map(t => t.trim()).includes(ext)) {
        externalViewers.value = viewers;
        if (!selectedExternalViewer.value) {
          selectedExternalViewer.value = Object.keys(viewers)[0];
        }
        const viewerUrl = viewers[selectedExternalViewer.value];
        // 创建带认证的URL
        const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
        const encodedUrl = encodeURIComponent(`${window.location.origin}${downloadUrl}`);
        externalViewerUrl.value = viewerUrl.replace('$e_url', encodedUrl);
      break;
    }
    }
  } catch (err) {
    error.value = '加载外部预览失败';
  }
}

function switchExternalViewer() {
  if (selectedExternalViewer.value && externalViewers.value[selectedExternalViewer.value]) {
    const template = externalViewers.value[selectedExternalViewer.value];
    // 创建带认证的URL
    const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
    const encodedUrl = encodeURIComponent(`${window.location.origin}${downloadUrl}`);
    const rawUrl = `${window.location.origin}${downloadUrl}`;
    externalViewerUrl.value = template.replace('$e_url', encodedUrl).replace('$url', rawUrl);
  }
}

function detectLanguage(ext) {
  const languageMap = {
    'js': 'javascript',
    'ts': 'typescript',
    'tsx': 'typescript',
    'py': 'python',
    'java': 'java',
    'html': 'html',
    'htm': 'html',
    'css': 'css',
    'json': 'json',
    'xml': 'xml',
    'sql': 'sql',
    'md': 'markdown',
    'yml': 'yaml',
    'yaml': 'yaml',
    'go': 'go',
    'rs': 'rust',
    'cpp': 'cpp',
    'c': 'c',
    'php': 'php',
    'vue': 'vue',
    'sh': 'shell',
    'bat': 'batch'
  };
  return languageMap[ext] || '';
}

// 简单的语法高亮函数
function highlightCode(code, language) {
  if (!language || !code) return code;
  
  // 基本的关键字高亮
  const keywords = {
    javascript: ['function', 'const', 'let', 'var', 'if', 'else', 'for', 'while', 'return', 'class', 'import', 'export', 'default', 'async', 'await', 'try', 'catch', 'finally', 'new', 'this', 'typeof', 'instanceof'],
    python: ['def', 'class', 'if', 'else', 'elif', 'for', 'while', 'return', 'import', 'from', 'as', 'try', 'except', 'finally', 'with', 'lambda', 'yield', 'and', 'or', 'not', 'in', 'is'],
    java: ['public', 'private', 'protected', 'class', 'interface', 'extends', 'implements', 'if', 'else', 'for', 'while', 'return', 'import', 'package', 'try', 'catch', 'finally', 'new', 'this', 'static', 'final'],
    html: ['html', 'head', 'body', 'div', 'span', 'p', 'a', 'img', 'ul', 'ol', 'li', 'table', 'tr', 'td', 'th', 'script', 'style', 'link', 'meta'],
    css: ['color', 'background', 'margin', 'padding', 'border', 'width', 'height', 'display', 'position', 'font', 'text', 'flex', 'grid', 'transform', 'transition'],
    sql: ['SELECT', 'FROM', 'WHERE', 'INSERT', 'UPDATE', 'DELETE', 'CREATE', 'DROP', 'ALTER', 'TABLE', 'INDEX', 'JOIN', 'LEFT', 'RIGHT', 'INNER', 'OUTER', 'GROUP', 'ORDER', 'BY']
  };
  
  const langKeywords = keywords[language] || [];
  let highlightedCode = code;
  
  // 转义HTML字符
  highlightedCode = highlightedCode
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
  
  // 高亮关键字
  langKeywords.forEach(keyword => {
    const regex = new RegExp(`\\b${keyword}\\b`, 'gi');
    highlightedCode = highlightedCode.replace(regex, `<span class="syntax-keyword">${keyword}</span>`);
  });
  
  // 高亮字符串
  highlightedCode = highlightedCode.replace(/(["'])((?:\\.|(?!\1)[^\\])*?)\1/g, '<span class="syntax-string">$1$2$1</span>');
  
  // 高亮注释
  if (language === 'javascript' || language === 'java' || language === 'css' || language === 'typescript') {
    highlightedCode = highlightedCode.replace(/\/\*[\s\S]*?\*\//g, '<span class="syntax-comment">$&</span>');
    highlightedCode = highlightedCode.replace(/\/\/.*$/gm, '<span class="syntax-comment">$&</span>');
  } else if (language === 'python') {
    highlightedCode = highlightedCode.replace(/#.*$/gm, '<span class="syntax-comment">$&</span>');
  } else if (language === 'html') {
    highlightedCode = highlightedCode.replace(/&lt;!--[\s\S]*?--&gt;/g, '<span class="syntax-comment">$&</span>');
  }
  
  // 高亮数字
  highlightedCode = highlightedCode.replace(/\b\d+\.?\d*\b/g, '<span class="syntax-number">$&</span>');
  
  return highlightedCode;
}

function onImageLoad() {
  // 图片加载完成
}

function onPreviewError() {
  error.value = '预览失败，请尝试下载文件';
}

function handleAudioCoverError(event) {
  // 音频封面加载失败时，使用默认图标
  event.target.src = '/favicon.ico';
  // 可以在这里添加用户提示
  console.warn('音频封面加载失败，已切换到默认图标');
}

// 下载文件
async function downloadFile() {
  try {
    // 创建一个带有认证信息的URL
    const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
    
    // 先检查文件是否可访问
    const checkRes = await axios.head(downloadUrl, {
      headers: {
        'x-username': user.value.username || 'guest'
      }
    });
    
    if (checkRes.status === 200) {
      // 使用a标签下载
      const a = document.createElement('a');
      a.href = downloadUrl;
      a.download = fileName.value;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    }
  } catch (err) {
    if (err.response?.status === 401) {
      error.value = '无权下载此文件，请先登录';
      notification.error('无权下载此文件，请先登录');
      // 可以选择跳转到登录页面
      // router.push('/login');
    } else {
      error.value = err.response?.data || '下载文件失败';
      notification.error('下载文件失败');
}
  }
}

// 复制链接
async function copyLink() {
  try {
    const downloadUrl = `/api/download?path=${encodeURIComponent(filePath.value)}&x-username=${encodeURIComponent(user.value.username || 'guest')}`;
    await navigator.clipboard.writeText(`${window.location.origin}${downloadUrl}`);
    notification.success('链接已复制到剪贴板');
  } catch (err) {
    notification.error('复制链接失败');
  }
}

function toggleDarkMode() {
  isDarkMode.value = !isDarkMode.value;
  localStorage.setItem('yaolist_dark_mode', isDarkMode.value.toString());
  
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode');
  } else {
    document.body.classList.remove('dark-mode');
  }
}

// 生命周期
onMounted(async () => {
  await loadSiteInfo();
  await loadFileInfo();
  
  // 应用保存的主题设置
  if (isDarkMode.value) {
    document.body.classList.add('dark-mode');
  }
});

watch(() => route.path, async () => {
  await loadFileInfo();
});
</script>

<style scoped>
.yaolist-flex-root {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  background: #f5f6fa;
  padding-left: 0 !important;
  background-size: cover;
  background-position: center;
  background-attachment: fixed;
  transition: background-image 0.3s ease;
}

/* 主题切换按钮容器 */
.theme-toggle-container {
  position: fixed;
  top: 20px;
  right: 20px;
  z-index: 1000;
}

/* 站点标题 - 位于文件卡片上方 */
.yaolist-title.title-above-card {
  margin: 0 0 24px 0;
  text-align: left;
  font-size: 3.3rem;
  font-weight: bold;
  letter-spacing: 2px;
  padding-left: 0;
  display: flex;
  align-items: center;
  max-width: 1100px;
  width: 100%;
}

.title-left {
  display: flex;
  align-items: center;
  gap: 16px;
}

.title-text {
  font-size: 3.3rem;
  font-weight: bold;
  letter-spacing: 2px;
  color: #2c3e50;
}

.theme-toggle-btn {
  background: rgba(255, 255, 255, 0.9);
  border: 1px solid rgba(0, 0, 0, 0.1);
  border-radius: 8px;
  padding: 10px;
  cursor: pointer;
  transition: all 0.3s ease;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #666;
  backdrop-filter: blur(10px);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
}

.theme-toggle-btn:hover {
  background: rgba(255, 255, 255, 1);
  border-color: rgba(0, 0, 0, 0.2);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  color: #333;
}

.yaolist-logo-large {
  vertical-align: middle;
  width: 60px;
  height: 60px;
  margin-right: 16px;
}

.yaolist-flex-center {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: flex-start;
  align-items: center;
  padding: 40px 20px 20px 20px;
}

.yaolist-card {
  border-radius: 32px !important;
  box-shadow: 0 4px 24px rgba(0,0,0,0.08);
  background: rgba(255, 255, 255, 0.7);
  padding: 32px 24px 24px 24px;
  margin: 0 auto 20px auto;
  min-width: 340px;
  max-width: 1100px;
  width: 100%;
  display: flex;
  flex-direction: column;
  min-height: auto;
  height: auto;
  /* 确保卡片不影响视频显示 */
  position: relative;
  z-index: 1;
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.3);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
}

.dark-mode .yaolist-card {
  background: rgba(30, 30, 30, 0.7);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.yaolist-path-breadcrumb {
  margin-bottom: 20px;
  padding: 12px 16px;
  background: #f8f9fa;
  border-radius: 8px;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
}

.yaolist-breadcrumb {
  color: #606266;
}

.yaolist-breadcrumb.clickable {
  color: #409EFF;
  cursor: pointer;
  transition: all 0.2s ease;
  border-radius: 6px;
  padding: 4px 8px;
}

.yaolist-breadcrumb.clickable:hover {
  color: #66b1ff;
  background: rgba(64, 158, 255, 0.1);
}

.yaolist-breadcrumb-sep {
  margin: 0 8px;
  color: #909399;
}

.file-info-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px 20px;
  background: #f8f9fa;
  border-radius: 8px;
  margin-bottom: 20px;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
}

.file-info-left {
  display: flex;
  align-items: center;
  gap: 16px;
}

.file-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  background: #fff;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}

.file-icon-svg {
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.file-icon-svg svg {
  width: 24px;
  height: 24px;
  stroke-width: 1.5;
}

.file-details {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.file-name {
  font-size: 18px;
  font-weight: 600;
  color: #303133;
  word-break: break-all;
}

.file-meta {
  display: flex;
  gap: 16px;
  font-size: 14px;
  color: #909399;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen', 'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif;
  font-weight: 500;
  letter-spacing: 0.025em;
}

.file-actions {
  display: flex;
  gap: 12px;
}

.action-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s;
  font-size: 14px;
  font-weight: 500;
}

.action-btn.download {
  background: #409eff;
  color: white;
}

.action-btn.download:hover {
  background: #66b1ff;
}

.action-btn.copy {
  background: #7c4dff;
  color: white;
}

.action-btn.copy:hover {
  background: #9575ff;
}

.file-preview-content {
  width: 100%;
  min-height: 400px;
  overflow: hidden;
}

.loading-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 60px;
}

.loading-spinner {
  width: 40px;
  height: 40px;
  border: 4px solid #f3f3f3;
  border-top: 4px solid #409eff;
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}

.loading-text {
  color: #606266;
  font-size: 16px;
}

/* 预览容器样式 */
.preview-image,
.preview-video,
.preview-audio,
.preview-text,
.preview-m3u8,
.preview-office,
.preview-pdf,
.preview-epub,
.preview-external,
.preview-error,
.preview-unsupported {
  width: 100%;
  background: rgba(255, 255, 255, 0.7) !important;
  border-radius: 12px;
  box-shadow: 0 2px 8px rgba(0,0,0,0.05);
  overflow: hidden;
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.3);
}

.dark-mode .preview-image,
.dark-mode .preview-video,
.dark-mode .preview-audio,
.dark-mode .preview-text,
.dark-mode .preview-m3u8,
.dark-mode .preview-office,
.dark-mode .preview-pdf,
.dark-mode .preview-epub,
.dark-mode .preview-external,
.dark-mode .preview-error,
.dark-mode .preview-unsupported {
  background: rgba(30, 30, 30, 0.7) !important;
  border: 1px solid rgba(255, 255, 255, 0.1);
}

/* 图片预览 */
.preview-image {
  text-align: center;
  padding: 20px;
}

.preview-image img {
  max-width: 100%;
  max-height: 70vh;
  object-fit: contain;
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.1);
}

/* 视频预览容器 */
.preview-video,
.preview-m3u8 {
  padding: 0;
  background: transparent;
  border: none;
  box-shadow: none;
  text-align: center;
}

/* 音频预览 */
.preview-audio {
  padding: 40px;
  text-align: center;
}

.audio-cover {
  margin-bottom: 24px;
}

.audio-cover img {
  width: 200px;
  height: 200px;
  border-radius: 12px;
  object-fit: cover;
  box-shadow: 0 4px 12px rgba(0,0,0,0.2);
}

.audio-info {
  display: flex;
  flex-direction: column;
  gap: 16px;
  align-items: center;
}

.audio-title {
  font-size: 18px;
  font-weight: 600;
  color: #303133;
}

.preview-audio audio {
  width: 100%;
  max-width: 400px;
}

/* 文本预览 */
.preview-text {
  padding: 24px;
  width: 100%;
  box-sizing: border-box;
}

.text-toolbar {
  margin-bottom: 16px;
  padding-bottom: 16px;
  border-bottom: 1px solid #e4e7ed;
}

.language-select {
  padding: 6px 12px;
  border: 1px solid #dcdfe6;
  border-radius: 4px;
  background: #fff;
  color: #606266;
}

.markdown-content {
  line-height: 1.6;
  color: #303133;
  max-width: 100%;
  overflow-x: auto;
}

.code-content {
  background: #f8f9fa;
  border: 1px solid #e4e7ed;
  border-radius: 6px;
  padding: 16px;
  overflow-x: auto;
  font-family: 'Fira Code', 'Monaco', 'Cascadia Code', 'Roboto Mono', 'Courier New', monospace;
  font-size: 14px;
  line-height: 1.5;
  color: #303133;
  white-space: pre;
  max-height: 70vh;
  overflow-y: auto;
  max-width: 100%;
  box-sizing: border-box;
}

/* 代码高亮样式 */
.code-content.javascript,
.code-content.js {
  background: #f8f8f2;
  color: #272822;
}

.code-content.python {
  background: #f8f8f2;
  color: #272822;
}

.code-content.html {
  background: #f8f8f2;
  color: #272822;
}

.code-content.css {
  background: #f8f8f2;
  color: #272822;
}

.code-content.json {
  background: #f8f8f2;
  color: #272822;
}

.code-content.xml {
  background: #f8f8f2;
  color: #272822;
}

.code-content.sql {
  background: #f8f8f2;
  color: #272822;
}

.code-content.markdown,
.code-content.md {
  background: #f8f8f2;
  color: #272822;
}

.code-content.yaml,
.code-content.yml {
  background: #f8f8f2;
  color: #272822;
}

.code-content.go {
  background: #f8f8f2;
  color: #272822;
}

.code-content.rust {
  background: #f8f8f2;
  color: #272822;
}

.code-content.cpp,
.code-content.c {
  background: #f8f8f2;
  color: #272822;
}

.code-content.php {
  background: #f8f8f2;
  color: #272822;
}

.code-content.vue {
  background: #f8f8f2;
  color: #272822;
}

.code-content.typescript,
.code-content.ts {
  background: #f8f8f2;
  color: #272822;
}

.code-content.shell,
.code-content.bash {
  background: #2d3748;
  color: #e2e8f0;
}

.code-content.batch {
  background: #1e1e1e;
  color: #d4d4d4;
}

/* 语法高亮样式 */
.code-content :deep(.syntax-keyword) {
  color: #0066cc;
  font-weight: bold;
}

.code-content :deep(.syntax-string) {
  color: #008000;
}

.code-content :deep(.syntax-comment) {
  color: #808080;
  font-style: italic;
}

.code-content :deep(.syntax-number) {
  color: #ff6600;
}

/* Office文档预览 */
.preview-office,
.preview-pdf,
.preview-epub,
.preview-external {
  height: 70vh;
  display: flex;
  flex-direction: column;
}

.office-toolbar,
.external-toolbar {
  padding: 16px;
  border-bottom: 1px solid #e4e7ed;
  background: #f8f9fa;
}

.viewer-select {
  padding: 6px 12px;
  border: 1px solid #dcdfe6;
  border-radius: 4px;
  background: #fff;
  color: #606266;
}

.preview-office iframe,
.preview-pdf iframe,
.preview-epub iframe,
.preview-external iframe {
  flex: 1;
  width: 100%;
  border: none;
}

/* 错误和不支持的类型 */
.preview-error,
.preview-unsupported {
  padding: 60px;
  text-align: center;
}

.error-icon,
.unsupported-icon {
  font-size: 48px;
  margin-bottom: 16px;
}

.error-message,
.unsupported-message {
  font-size: 18px;
  color: #606266;
  margin-bottom: 24px;
}

.download-btn {
  padding: 12px 24px;
  background: #409eff;
  color: white;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  font-size: 16px;
  transition: all 0.2s;
}

.download-btn:hover {
  background: #66b1ff;
}

.yaolist-bottom-userinfo.userinfo-float {
  margin: 32px auto 32px auto;
  text-align: center;
  font-size: 1.1rem;
  color: #888;
  max-width: 1100px;
  width: 100%;
  display: flex;
  justify-content: center;
  align-items: center;
}

.userinfo-action {
  transition: all 0.2s ease;
}

.userinfo-action:hover {
  color: #66b1ff !important;
}

/* 响应式设计 */
@media (max-width: 768px) {
  .yaolist-flex-root {
    padding: 16px;
  }
  
  .yaolist-card {
    margin: 0 auto 16px auto;
    padding: 20px 16px 16px 16px;
    border-radius: 16px !important;
  }
  
  .file-info-header {
    flex-direction: column;
    gap: 16px;
    align-items: stretch;
  }
  
  .file-info-left {
    justify-content: center;
  }
  
  .file-actions {
    justify-content: center;
  }
  
  .audio-cover img {
    width: 150px;
    height: 150px;
  }
  
  .preview-office,
  .preview-pdf,
  .preview-epub {
    height: 60vh;
  }
}

/* 视频播放器样式 */
.video-player {
  width: 100%;
  max-width: 100%;
  height: auto;
  max-height: 70vh;
  background: #000;
  border: none;
  outline: none;
  display: block;
  margin: 0 auto;
}

/* 移除Element Plus可能的干扰 */
.preview-video .el-loading-mask,
.preview-m3u8 .el-loading-mask,
.preview-video .el-overlay,
.preview-m3u8 .el-overlay {
  display: none !important;
}

/* 深色模式样式 */
.dark-mode {
  background: #1a1a1a !important;
}

.dark-mode .title-text {
  color: #e0e0e0 !important;
}

.dark-mode .theme-toggle-container {
  background: transparent;
}

.dark-mode .theme-toggle-btn {
  background: rgba(45, 45, 45, 0.9) !important;
  border-color: rgba(255, 255, 255, 0.2) !important;
  color: #b0b0b0 !important;
}

.dark-mode .theme-toggle-btn:hover {
  background: rgba(60, 60, 60, 1) !important;
  border-color: rgba(255, 255, 255, 0.3) !important;
  color: #e0e0e0 !important;
}

.dark-mode .yaolist-card {
  background: #2d2d2d !important;
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.3) !important;
}

.dark-mode .yaolist-path-breadcrumb {
  background: #3a3a3a !important;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2) !important;
}

.dark-mode .yaolist-breadcrumb {
  color: #b0b0b0 !important;
}

.dark-mode .yaolist-breadcrumb.clickable {
  color: #66b1ff !important;
}

.dark-mode .yaolist-breadcrumb.clickable:hover {
  background: rgba(102, 177, 255, 0.2) !important;
}

.dark-mode .file-info-header {
  background: #3a3a3a !important;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2) !important;
}

.dark-mode .file-icon {
  background: #4a4a4a !important;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3) !important;
}

.dark-mode .file-name {
  color: #e0e0e0 !important;
}

.dark-mode .file-meta {
  color: #b0b0b0 !important;
}

.dark-mode .loading-text {
  color: #e0e0e0 !important;
}

.dark-mode .audio-title {
  color: #e0e0e0 !important;
}

.dark-mode .text-toolbar {
  border-bottom-color: #4a4a4a !important;
}

.dark-mode .language-select,
.dark-mode .viewer-select {
  background: #3a3a3a !important;
  border-color: #4a4a4a !important;
  color: #e0e0e0 !important;
}

.dark-mode .preview-text {
  background: #2d2d2d !important;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3) !important;
}

.dark-mode .preview-image,
.dark-mode .preview-audio,
.dark-mode .preview-office,
.dark-mode .preview-pdf,
.dark-mode .preview-epub,
.dark-mode .preview-error,
.dark-mode .preview-unsupported {
  background: #2d2d2d !important;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3) !important;
}

.dark-mode .markdown-content {
  color: #e0e0e0 !important;
}

.dark-mode .code-content {
  background: #1e1e1e !important;
  border-color: #4a4a4a !important;
  color: #d4d4d4 !important;
}

/* 深色模式下的代码高亮 */
.dark-mode .code-content.javascript,
.dark-mode .code-content.js {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.python {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.html {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.css {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.json {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.xml {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.sql {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.markdown,
.dark-mode .code-content.md {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.yaml,
.dark-mode .code-content.yml {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.go {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.rust {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.cpp,
.dark-mode .code-content.c {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.php {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.vue {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.typescript,
.dark-mode .code-content.ts {
  background: #1e1e1e !important;
  color: #d4d4d4 !important;
}

.dark-mode .code-content.shell,
.dark-mode .code-content.bash {
  background: #0d1117 !important;
  color: #c9d1d9 !important;
}

.dark-mode .code-content.batch {
  background: #0d1117 !important;
  color: #c9d1d9 !important;
}

/* 深色模式下的语法高亮 */
.dark-mode .code-content :deep(.syntax-keyword) {
  color: #569cd6 !important;
  font-weight: bold;
}

.dark-mode .code-content :deep(.syntax-string) {
  color: #ce9178 !important;
}

.dark-mode .code-content :deep(.syntax-comment) {
  color: #6a9955 !important;
  font-style: italic;
}

.dark-mode .code-content :deep(.syntax-number) {
  color: #b5cea8 !important;
}

.dark-mode .office-toolbar,
.dark-mode .external-toolbar {
  background: #3a3a3a !important;
  border-bottom-color: #4a4a4a !important;
}

.dark-mode .error-message,
.dark-mode .unsupported-message {
  color: #b0b0b0 !important;
}

.dark-mode .yaolist-bottom-userinfo {
  color: #b0b0b0 !important;
}

.dark-mode .userinfo-username,
.dark-mode .userinfo-action {
  color: #66b1ff !important;
}

.dark-mode .userinfo-action:hover {
  color: #409EFF !important;
}

</style> 