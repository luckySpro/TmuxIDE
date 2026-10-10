import { createApp } from 'vue'
import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/600.css'
import '@xterm/xterm/css/xterm.css'
import './styles.css'
import App from './App.vue'
import { captureToken } from './api'

captureToken()
createApp(App).mount('#app')
