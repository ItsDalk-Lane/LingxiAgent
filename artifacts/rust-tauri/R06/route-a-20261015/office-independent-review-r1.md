# Office 独立复核：NEEDS_FIX

P1：普通Excel日期被悄悄显示为序号。用现役ExcelJS创建Date(2026-10-14T09:30:00Z)、日期样式yyyy-mm-dd hh:mm:ss，再由ExcelJS重新载入取cell.text，得到Wed Oct 14 2026 17:30:00 GMT+0800 (China Standard Time)。同一文件通过冻结当前CLI真实GET /lingxi/v1/fs/xlsx-html得到200，单元格却为46309.39583333333。源码xlsx未读取styles/date1904，直接输出v数值。应保留日期类型与真实值的文本表示，并增加1900/1904及公式缓存日期验证，不能用现有5PASS替代。

已独立核对13源SHA完全相符，原父5项重跑5/5通过12.57秒。另独立真实CLI检查公式缓存5、错误#DIV/0!、共享富文本与实体文本顺序、Word标题及表格；外部实体/坏包/无凭证/授权根外路径明确拒绝，全部源文件SHA不变。

真实ExcelJS源文件、生成脚本、重读期望、原请求结果位于/private/tmp/r06-office-cross-independent；完整机器报告/private/tmp/r06-office-cross-independent-review.json。首探针将HTML误按JSON解析的失败保留，属于夹具问题；日期失败在纠正后的实际响应确认。未修改Main、登记、门禁、原断言。
