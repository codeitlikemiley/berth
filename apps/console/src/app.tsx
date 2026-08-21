import { type ReactNode } from "react";
import { Navigate, Route, Routes } from "react-router-dom";

import { getToken } from "@/lib/auth";
import { DoctorPage } from "@/pages/doctor";
import { HomePage } from "@/pages/home";
import { LeasePage } from "@/pages/lease";
import { PairPage } from "@/pages/pair";

function RequireAuth({ children }: { children: ReactNode }) {
  if (!getToken()) {
    return <Navigate to="/pair" replace />;
  }
  return children;
}

export function App() {
  return (
    <Routes>
      <Route path="/pair" element={<PairPage />} />
      <Route
        path="/"
        element={
          <RequireAuth>
            <HomePage />
          </RequireAuth>
        }
      />
      <Route path="/leases/new" element={<Navigate to="/" replace />} />
      <Route
        path="/leases/:id"
        element={
          <RequireAuth>
            <LeasePage />
          </RequireAuth>
        }
      />
      <Route
        path="/doctor"
        element={
          <RequireAuth>
            <DoctorPage />
          </RequireAuth>
        }
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
