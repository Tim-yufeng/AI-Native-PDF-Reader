import { mount } from 'svelte';
import App from './App.svelte';
import 'pdfjs-dist/web/pdf_viewer.css';
import './app.css';
import './lib/freetext-control.css';

mount(App, { target: document.getElementById('app')! });
