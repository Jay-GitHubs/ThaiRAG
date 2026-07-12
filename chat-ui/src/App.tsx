import { Routes, Route } from 'react-router-dom';
import { LoginPage } from './pages/LoginPage';
import { ChatPage } from './pages/ChatPage';
import { ProtectedRoute } from './auth/ProtectedRoute';

export function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      {/* One catch-all route so "/" and "/c/{id}" share a single ChatPage
          instance — sibling routes would remount it (and refetch everything)
          on every conversation switch. ChatPage parses the /c/{id} path and
          normalizes unknown paths back to "/". */}
      <Route
        path="*"
        element={
          <ProtectedRoute>
            <ChatPage />
          </ProtectedRoute>
        }
      />
    </Routes>
  );
}
