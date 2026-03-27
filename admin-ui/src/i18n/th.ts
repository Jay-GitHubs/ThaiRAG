const th: Record<string, string> = {
  // Sidebar menu items
  'menu.dashboard': 'แดชบอร์ด',
  'menu.kmHierarchy': 'โครงสร้างฐานความรู้',
  'menu.documents': 'เอกสาร',
  'menu.knowledgeGraph': 'กราฟความรู้',
  'menu.testChat': 'ทดสอบแชท',
  'menu.users': 'ผู้ใช้งาน',
  'menu.permissions': 'สิทธิ์การเข้าถึง',
  'menu.usageCosts': 'การใช้งานและค่าใช้จ่าย',
  'menu.feedbackTuning': 'ข้อเสนอแนะและการปรับแต่ง',
  'menu.analytics': 'การวิเคราะห์',
  'menu.connectors': 'ตัวเชื่อมต่อ',
  'menu.inferenceLogs': 'บันทึกการอนุมาน',
  'menu.searchEval': 'ประเมินการค้นหา',
  'menu.abTesting': 'การทดสอบ A/B',
  'menu.backupRestore': 'สำรองและกู้คืนข้อมูล',
  'menu.settings': 'ตั้งค่า',
  'menu.health': 'สถานะระบบ',

  // Header
  'header.loggedInAs': 'เข้าสู่ระบบเป็น {email}',
  'header.logout': 'ออกจากระบบ',
  'header.lightMode': 'เปลี่ยนเป็นโหมดสว่าง',
  'header.darkMode': 'เปลี่ยนเป็นโหมดมืด',

  // Common actions
  'action.save': 'บันทึก',
  'action.cancel': 'ยกเลิก',
  'action.delete': 'ลบ',
  'action.search': 'ค้นหา',
  'action.upload': 'อัปโหลด',
  'action.edit': 'แก้ไข',
  'action.create': 'สร้าง',
  'action.refresh': 'รีเฟรช',
  'action.close': 'ปิด',
  'action.confirm': 'ยืนยัน',
  'action.submit': 'ส่ง',
  'action.reset': 'รีเซ็ต',
  'action.back': 'กลับ',
  'action.next': 'ถัดไป',
  'action.enable': 'เปิดใช้งาน',
  'action.disable': 'ปิดใช้งาน',

  // Status labels
  'status.active': 'ใช้งาน',
  'status.disabled': 'ปิดใช้งาน',
  'status.queued': 'รอคิว',
  'status.running': 'กำลังทำงาน',
  'status.completed': 'เสร็จสมบูรณ์',
  'status.failed': 'ล้มเหลว',
  'status.pending': 'รอดำเนินการ',

  // Common messages
  'message.noData': 'ไม่มีข้อมูล',
  'message.loading': 'กำลังโหลด...',
  'message.error': 'เกิดข้อผิดพลาด',
  'message.success': 'ดำเนินการสำเร็จ',
  'message.confirmDelete': 'คุณแน่ใจหรือไม่ว่าต้องการลบ?',
  'message.cannotUndo': 'การดำเนินการนี้ไม่สามารถย้อนกลับได้',

  // Documents page
  'documents.title': 'เอกสาร',
  'documents.selectOrg': 'เลือกองค์กร',
  'documents.selectDept': 'เลือกแผนก',
  'documents.selectWorkspace': 'เลือกพื้นที่ทำงาน',
  'documents.selectPrompt': 'เลือกองค์กร แผนก และพื้นที่ทำงานเพื่อดูเอกสาร',

  // Users page
  'users.title': 'จัดการผู้ใช้งาน',
  'users.searchPlaceholder': 'ค้นหาด้วยชื่อหรืออีเมล...',
  'users.userCount': '{count} ผู้ใช้',
  'users.deleteUser': 'ลบผู้ใช้นี้?',
  'users.disableUser': 'ปิดใช้งานผู้ใช้นี้?',
  'users.enableUser': 'เปิดใช้งานผู้ใช้นี้?',
  'users.disableDescription': 'ผู้ใช้จะไม่สามารถเข้าสู่ระบบได้',
  'users.enableDescription': 'ผู้ใช้จะสามารถเข้าสู่ระบบได้อีกครั้ง',
  'users.deleted': 'ลบผู้ใช้แล้ว',
  'users.deleteFailed': 'ไม่สามารถลบผู้ใช้ได้',
  'users.roleUpdated': 'อัปเดตบทบาทแล้ว',
  'users.roleUpdateFailed': 'ไม่สามารถอัปเดตบทบาทได้',
  'users.enabled': 'เปิดใช้งานผู้ใช้แล้ว',
  'users.disabled': 'ปิดใช้งานผู้ใช้แล้ว',
  'users.statusUpdateFailed': 'ไม่สามารถอัปเดตสถานะได้',

  // Table columns
  'column.name': 'ชื่อ',
  'column.email': 'อีเมล',
  'column.provider': 'ผู้ให้บริการ',
  'column.role': 'บทบาท',
  'column.status': 'สถานะ',
  'column.created': 'สร้างเมื่อ',
  'column.userId': 'รหัสผู้ใช้',
  'column.actions': 'การดำเนินการ',

  // Roles
  'role.viewer': 'ผู้ดู',
  'role.editor': 'ผู้แก้ไข',
  'role.admin': 'ผู้ดูแลระบบ',
  'role.superAdmin': 'ผู้ดูแลระบบสูงสุด',
};

export default th;
