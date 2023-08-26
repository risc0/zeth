// src/App.tsx

import React from 'react';
import './App.css';
import VerificationForm from './VerificationForm';

const App: React.FC = () => {
  return (
    <div className="App">
      <header className="App-header">
        <VerificationForm />
      </header>
    </div>
  );
}

export default App;