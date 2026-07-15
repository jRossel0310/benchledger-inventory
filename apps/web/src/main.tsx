import { generateCssVariables } from '@ei/shared';
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './web.css';

const style = document.createElement('style');
style.textContent = generateCssVariables('dark');
document.head.appendChild(style);

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
