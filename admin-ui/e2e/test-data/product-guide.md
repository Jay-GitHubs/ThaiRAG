# DataFlow Pro — Complete User Guide

**Product Version:** 5.4
**Documentation Version:** 5.4.1
**Last Updated:** March 2026
**Publisher:** DataFlow Systems, Inc.

---

## Table of Contents

1. Introduction to DataFlow Pro
2. Getting Started and Onboarding
3. Dashboard Overview
4. Data Import and Export
5. Report Generation
6. User Management and Roles
7. API Integration Guide
8. Workflows and Automation
9. Advanced Configurations
10. Security and Compliance
11. Troubleshooting Common Issues
12. Appendices

---

## 1. Introduction to DataFlow Pro

### 1.1 What is DataFlow Pro?

DataFlow Pro is an enterprise-grade data management and analytics platform designed to help organizations collect, transform, analyze, and visualize data from multiple sources in real time. Whether you are a data analyst building reports, an engineer integrating APIs, or a business leader seeking actionable insights, DataFlow Pro provides the tools you need to make data-driven decisions with confidence.

DataFlow Pro serves organizations across industries including finance, healthcare, retail, manufacturing, and technology. The platform processes over 50 billion data events daily across its customer base and maintains 99.99% uptime through its globally distributed infrastructure.

### 1.2 Key Features

DataFlow Pro offers a comprehensive suite of features organized into the following categories:

**Data Integration:**
- Connect to 200+ data sources including databases, APIs, file systems, and SaaS applications
- Real-time streaming ingestion with sub-second latency
- Batch import support for CSV, JSON, XML, Parquet, Avro, and Excel formats
- Built-in data quality checks and validation rules
- Change Data Capture (CDC) for database synchronization

**Data Transformation:**
- Visual drag-and-drop pipeline builder for no-code transformations
- SQL editor with autocomplete and syntax highlighting for advanced users
- Python and R script execution for custom transformations
- Pre-built transformation templates for common use cases
- Schema evolution management with automatic type coercion

**Analytics and Reporting:**
- Interactive dashboards with 40+ chart types and visualization options
- Scheduled reports with email and Slack delivery
- Ad-hoc query builder for self-service analytics
- Embedded analytics for integrating reports into external applications
- Natural language query support powered by AI

**Collaboration:**
- Shared workspaces with role-based access control
- Version-controlled data assets with audit trails
- Comments, annotations, and @mentions on reports and dashboards
- Collaborative query editing with real-time co-authoring

### 1.3 System Requirements

To use DataFlow Pro effectively, ensure your environment meets the following requirements:

| Component | Minimum Requirement | Recommended |
|-----------|-------------------|-------------|
| Browser | Chrome 110+, Firefox 115+, Safari 17+, Edge 110+ | Latest stable version |
| Screen Resolution | 1280 x 720 | 1920 x 1080 or higher |
| Internet Speed | 5 Mbps download | 25 Mbps or higher |
| RAM (for Desktop Agent) | 4 GB | 8 GB or higher |
| Disk Space (for Desktop Agent) | 500 MB | 2 GB |
| Operating System (Desktop Agent) | Windows 10+, macOS 12+, Ubuntu 20.04+ | Latest stable version |

### 1.4 Supported Browsers and Limitations

DataFlow Pro is a web-based application that runs entirely in the browser for most use cases. The Desktop Agent is required only for connecting to on-premises data sources that are not accessible from the internet.

Browser-specific notes:
- **Chrome:** Full support. Recommended for best performance with large datasets.
- **Firefox:** Full support. Hardware acceleration should be enabled for optimal chart rendering.
- **Safari:** Full support. Some keyboard shortcuts may differ from Chrome/Firefox.
- **Edge:** Full support. Chromium-based Edge 110+ is required.
- **Mobile browsers:** Read-only dashboard viewing is supported on iOS Safari and Android Chrome. Editing and configuration features are not available on mobile.

---

## 2. Getting Started and Onboarding

### 2.1 Creating Your Account

If your organization has already set up DataFlow Pro, you will receive an invitation email from your administrator. Follow these steps to create your account:

1. Open the invitation email and click the **"Accept Invitation"** button
2. You will be redirected to the DataFlow Pro registration page
3. Enter your full name and create a password that meets the following requirements:
   - At least 12 characters long
   - Contains at least one uppercase letter, one lowercase letter, one number, and one special character
   - Does not contain your name or email address
4. Configure multi-factor authentication (MFA) by scanning the QR code with your authenticator app (Google Authenticator, Authy, or Microsoft Authenticator)
5. Click **"Create Account"** to complete registration
6. You will be redirected to the onboarding wizard

If you are the first user in your organization (the organization administrator), visit https://app.dataflowpro.com/signup to create your organization and admin account.

### 2.2 Onboarding Wizard

The onboarding wizard guides new users through the initial setup process. It consists of the following steps:

**Step 1: Organization Profile**
- Enter your organization name, industry, and size
- Upload your company logo (displayed in reports and dashboards)
- Select your default time zone and date format
- Choose your preferred language (19 languages supported)

**Step 2: Connect Your First Data Source**
- Select a data source type from the catalog
- Enter connection credentials and test the connection
- DataFlow Pro will automatically discover available tables, schemas, and fields
- Select the tables and fields you want to import

**Step 3: Build Your First Dashboard**
- Choose from a library of pre-built dashboard templates based on your industry
- Or start with a blank canvas and add charts manually
- The wizard will suggest relevant metrics based on your data source

**Step 4: Invite Team Members**
- Enter email addresses of colleagues you want to invite
- Assign roles (Viewer, Analyst, Editor, or Admin)
- Customize the invitation message

**Step 5: Review and Launch**
- Review your configuration settings
- Click **"Launch DataFlow Pro"** to complete onboarding
- You will be taken to your new dashboard

### 2.3 Navigating the Interface

The DataFlow Pro interface is organized into the following main areas:

**Top Navigation Bar:**
- **Logo/Home:** Click to return to the main dashboard
- **Search:** Global search across all data assets, reports, and configurations
- **Notifications:** Alerts for scheduled reports, data pipeline status, and team mentions
- **Help:** Access documentation, tutorials, and support
- **Profile:** Account settings, preferences, and sign out

**Left Sidebar:**
- **Dashboards:** View and manage your dashboards and reports
- **Data Sources:** Connect and manage data source connections
- **Pipelines:** Build and monitor data transformation pipelines
- **Queries:** Write and manage SQL queries
- **Workspace:** Shared assets and collaborative spaces
- **Alerts:** Configure and manage data alerts and notifications
- **Settings:** Organization and account settings (admin only)

**Main Content Area:**
- Displays the selected page content (dashboard, query editor, pipeline builder, etc.)
- Supports tabbed navigation for working on multiple items simultaneously
- Responsive layout that adapts to your screen size

**Status Bar (Bottom):**
- Current connection status
- Active query execution progress
- Data refresh timestamps

### 2.4 Quick Start Tutorial

Follow this quick start tutorial to create your first analysis in DataFlow Pro:

**Step 1: Import Sample Data**
1. Navigate to **Data Sources** in the left sidebar
2. Click **"+ New Connection"**
3. Select **"Sample Data"** from the connection type list
4. Choose the **"E-Commerce Sales"** sample dataset
5. Click **"Connect"** — the data will be available immediately

**Step 2: Explore the Data**
1. Navigate to **Queries** in the left sidebar
2. Click **"+ New Query"**
3. In the query editor, type:
```sql
SELECT
    category,
    COUNT(*) as order_count,
    SUM(revenue) as total_revenue,
    AVG(revenue) as avg_order_value
FROM ecommerce_sales
WHERE order_date >= '2025-01-01'
GROUP BY category
ORDER BY total_revenue DESC
LIMIT 10;
```
4. Click **"Run"** (or press Ctrl+Enter / Cmd+Enter) to execute the query
5. Review the results in the table below the editor

**Step 3: Create a Visualization**
1. With your query results displayed, click **"Visualize"** in the results toolbar
2. Select **"Bar Chart"** from the chart type selector
3. Configure the chart:
   - X-Axis: `category`
   - Y-Axis: `total_revenue`
   - Color: `category`
4. Click **"Save to Dashboard"**
5. Enter a name for the chart (e.g., "Revenue by Category")
6. Select an existing dashboard or create a new one
7. Click **"Save"**

**Step 4: Share Your Dashboard**
1. Open the dashboard you just created
2. Click the **"Share"** button in the top-right corner
3. Enter the email addresses of colleagues you want to share with
4. Select the permission level (View or Edit)
5. Click **"Share"**

---

## 3. Dashboard Overview

### 3.1 Dashboard Layout

Dashboards in DataFlow Pro are organized using a flexible grid layout system. Each dashboard is composed of tiles, where each tile contains a visualization, text block, or embedded content. Tiles can be resized and repositioned by dragging and dropping.

**Grid System:**
- The dashboard canvas uses a 24-column grid
- Tiles can span any number of columns (minimum 4)
- Row height automatically adjusts based on content
- Responsive breakpoints adjust the layout for different screen sizes

**Dashboard Components:**
- **Charts:** Bar, line, area, pie, donut, scatter, bubble, heatmap, treemap, funnel, gauge, waterfall, histogram, box plot, radar, map, and more
- **Tables:** Interactive data tables with sorting, filtering, and pagination
- **Text Blocks:** Markdown-formatted text for titles, descriptions, and annotations
- **Filters:** Interactive filter controls (dropdowns, date pickers, sliders, text search)
- **Embedded Content:** iFrames for embedding external content or other DataFlow Pro dashboards
- **Images:** Static images for branding, diagrams, or decorative elements

### 3.2 Creating a Dashboard

To create a new dashboard:

1. Navigate to **Dashboards** in the left sidebar
2. Click **"+ New Dashboard"**
3. Enter a name and optional description for the dashboard
4. Select a folder to organize the dashboard (or create a new folder)
5. Choose a template or start with a blank canvas
6. Click **"Create"**

You will be taken to the dashboard editor, where you can add tiles and configure the layout.

### 3.3 Adding Charts

To add a chart to your dashboard:

1. Click the **"+ Add Tile"** button in the dashboard editor toolbar
2. Select **"Chart"** from the tile type options
3. Choose the data source for the chart:
   - **Saved Query:** Use a previously saved SQL query
   - **New Query:** Write a new query in the inline editor
   - **Dataset:** Select from a pre-configured dataset
4. Select the chart type from the visualization gallery
5. Configure the chart settings:

**Data Configuration:**
```
X-Axis:     Select the field for the horizontal axis
Y-Axis:     Select one or more fields for the vertical axis
Color:      Optional field for color-coding data series
Size:       Optional field for sizing data points (scatter/bubble)
Tooltip:    Fields to display when hovering over data points
Filters:    Add query-level filters to narrow the data
Sort:       Define the default sort order
Limit:      Maximum number of data points to display
```

**Appearance Configuration:**
```
Title:          Chart title displayed above the visualization
Subtitle:       Optional subtitle for additional context
Legend:         Position (top, bottom, left, right) and visibility
Axis Labels:    Custom labels for X and Y axes
Number Format:  Currency, percentage, decimal places, abbreviation
Color Palette:  Select from pre-defined palettes or create custom
Grid Lines:     Show/hide horizontal and vertical grid lines
Animation:      Enable/disable chart animations
```

6. Click **"Apply"** to add the chart to the dashboard
7. Resize and reposition the tile as needed

### 3.4 Interactive Filters

Filters allow dashboard viewers to dynamically explore data without modifying the underlying queries. DataFlow Pro supports the following filter types:

| Filter Type | Description | Best For |
|------------|-------------|----------|
| Dropdown | Single or multi-select from a list of values | Categorical fields with limited options |
| Date Picker | Select a date or date range | Date/time fields |
| Slider | Select a numeric range | Numeric fields |
| Text Search | Free-text search with autocomplete | Text fields with many unique values |
| Toggle | On/off switch for boolean filters | Boolean fields |
| Hierarchy | Drill-down through hierarchical categories | Geographic or organizational hierarchies |

**Creating a Dashboard Filter:**
1. Click **"+ Add Filter"** in the dashboard editor toolbar
2. Select the filter type
3. Configure the filter:
   - **Label:** Display name for the filter
   - **Data Source:** The field to filter on
   - **Default Value:** Pre-selected value when the dashboard loads
   - **Linked Charts:** Select which charts are affected by this filter
   - **Cascading:** Enable if this filter should update options in other filters
4. Click **"Add"** to place the filter on the dashboard

### 3.5 Dashboard Scheduling

DataFlow Pro can automatically refresh and deliver dashboards on a schedule:

1. Open the dashboard you want to schedule
2. Click **"Schedule"** in the dashboard toolbar
3. Configure the schedule:

```yaml
Frequency: Daily | Weekly | Monthly | Custom Cron
Time: 08:00 AM (UTC-5)
Time Zone: America/New_York
Delivery Method:
  - Email: recipient1@company.com, recipient2@company.com
  - Slack: #analytics-channel
  - Webhook: https://api.company.com/webhook/reports
Format: PDF | PNG | CSV (data only)
Include Filters: Yes (apply current filter selections)
```

4. Click **"Save Schedule"**

Scheduled dashboards will be rendered with the most recent data at the specified time and delivered to the configured recipients. You can view the delivery history and status in the **Schedule Log** tab.

### 3.6 Dashboard Permissions

Dashboard permissions control who can view, edit, and manage dashboards:

| Permission Level | Can View | Can Edit | Can Share | Can Delete |
|-----------------|----------|----------|-----------|------------|
| Viewer | Yes | No | No | No |
| Commenter | Yes | No (can add comments) | No | No |
| Editor | Yes | Yes | Yes | No |
| Owner | Yes | Yes | Yes | Yes |

Permissions can be set at the individual user level, team level, or organization level. Dashboard folders inherit permissions from their parent folder by default, but permissions can be overridden at the individual dashboard level.

---

## 4. Data Import and Export

### 4.1 Supported Data Sources

DataFlow Pro connects to over 200 data sources across the following categories:

**Databases:**
- PostgreSQL, MySQL, Microsoft SQL Server, Oracle, IBM DB2
- Amazon Redshift, Google BigQuery, Snowflake, Databricks
- MongoDB, Cassandra, DynamoDB, Elasticsearch
- SQLite, MariaDB, CockroachDB, TimescaleDB

**Cloud Storage:**
- Amazon S3, Google Cloud Storage, Azure Blob Storage
- SFTP/FTP servers
- Dropbox Business, Box, Google Drive, OneDrive

**SaaS Applications:**
- Salesforce, HubSpot, Marketo, Pardot
- Stripe, PayPal, Square, Braintree
- Google Analytics, Facebook Ads, LinkedIn Ads
- Jira, Asana, Monday.com, Trello
- Zendesk, Intercom, Freshdesk
- Shopify, WooCommerce, Magento
- Workday, BambooHR, ADP

**Streaming:**
- Apache Kafka, Amazon Kinesis, Google Pub/Sub
- RabbitMQ, Azure Event Hubs
- Custom webhooks and WebSocket connections

**File Formats:**
- CSV, TSV, JSON, JSON Lines, XML
- Apache Parquet, Apache Avro, ORC
- Microsoft Excel (.xlsx, .xls)
- Google Sheets

### 4.2 Connecting a Data Source

To connect a new data source:

1. Navigate to **Data Sources** in the left sidebar
2. Click **"+ New Connection"**
3. Search for or browse to your data source type
4. Enter the connection details:

**Example: PostgreSQL Connection**
```
Connection Name:    Production Database
Host:               db.company.com
Port:               5432
Database:           analytics
Schema:             public
Username:           dataflow_reader
Password:           ••••••••••••
SSL Mode:           Require
SSH Tunnel:         Enabled
  SSH Host:         bastion.company.com
  SSH Port:         22
  SSH Username:     dataflow
  SSH Key:          [Upload private key]
```

5. Click **"Test Connection"** to verify the connection
6. If the test succeeds, click **"Save Connection"**
7. DataFlow Pro will begin discovering tables, views, and schemas

### 4.3 Data Import Options

**Batch Import:**
Batch imports load data from files or external systems on a one-time or scheduled basis.

To perform a batch import:
1. Navigate to **Data Sources > Imports**
2. Click **"+ New Import"**
3. Select the source (file upload, cloud storage, or connected data source)
4. Configure import settings:

```yaml
Source: s3://company-data-lake/sales/2025/
File Format: CSV
Delimiter: ","
Header Row: Yes (first row)
Encoding: UTF-8
Null Values: "NULL", "", "N/A"
Date Format: YYYY-MM-DD
Timestamp Format: YYYY-MM-DD HH:mm:ss
Skip Rows: 0
Max Errors: 100
On Error: Skip Row | Abort Import
```

5. Preview the data and review detected column types
6. Adjust column types if necessary:

| Column Name | Detected Type | Override Type | Transform |
|------------|--------------|---------------|-----------|
| order_id | String | String | None |
| order_date | String | Date | Parse: YYYY-MM-DD |
| customer_id | Integer | Integer | None |
| amount | String | Decimal(10,2) | Remove currency symbol |
| status | String | Enum | Map: 1=Active, 2=Cancelled |

7. Select the destination table (create new or append to existing)
8. Click **"Start Import"**

**Real-Time Streaming:**
For real-time data ingestion, configure a streaming connection:

1. Navigate to **Data Sources > Streaming**
2. Click **"+ New Stream"**
3. Select the streaming source (Kafka, Kinesis, Webhooks, etc.)
4. Configure the stream:

```json
{
  "stream_name": "order_events",
  "source": {
    "type": "kafka",
    "bootstrap_servers": "kafka-1.company.com:9092,kafka-2.company.com:9092",
    "topic": "orders.created",
    "consumer_group": "dataflow-pro",
    "start_offset": "latest",
    "security_protocol": "SASL_SSL",
    "sasl_mechanism": "SCRAM-SHA-256"
  },
  "schema": {
    "format": "json",
    "schema_registry": "https://schema-registry.company.com"
  },
  "destination": {
    "table": "order_events",
    "partitioning": {
      "field": "event_timestamp",
      "granularity": "day"
    }
  },
  "processing": {
    "deduplication_key": "event_id",
    "watermark_delay": "5m",
    "checkpoint_interval": "1m"
  }
}
```

5. Click **"Start Stream"** to begin ingesting data

### 4.4 Data Export

DataFlow Pro supports exporting data in multiple formats:

**Manual Export:**
1. Run a query or open a dashboard with the data you want to export
2. Click the **"Export"** button
3. Select the export format:
   - **CSV:** Comma-separated values, compatible with Excel and most tools
   - **JSON:** JavaScript Object Notation, structured data format
   - **Parquet:** Columnar format, efficient for large datasets
   - **Excel:** Microsoft Excel workbook with formatting
   - **PDF:** Formatted report with charts and tables
4. Configure export options (delimiter, encoding, compression)
5. Click **"Download"** or **"Send to Cloud Storage"**

**Scheduled Export:**
Configure automated exports on a schedule:

```yaml
Export Name: Weekly Sales Report
Query: saved_query://weekly_sales_summary
Schedule: Every Monday at 6:00 AM UTC
Format: CSV
Compression: gzip
Destination:
  Type: S3
  Bucket: company-reports
  Path: sales/weekly/{YYYY}/{MM}/report_{YYYY-MM-DD}.csv.gz
  IAM Role: arn:aws:iam::123456789:role/DataFlowExport
Notification:
  On Success: slack://analytics-team
  On Failure: email://admin@company.com
```

**API Export:**
Use the DataFlow Pro API to programmatically export data:

```bash
curl -X POST "https://api.dataflowpro.com/v1/exports" \
  -H "Authorization: Bearer YOUR_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query_id": "qry_abc123",
    "format": "json",
    "filters": {
      "date_range": {
        "start": "2025-01-01",
        "end": "2025-12-31"
      }
    },
    "limit": 100000
  }'
```

---

## 5. Report Generation

### 5.1 Report Types

DataFlow Pro supports several report types to meet different analytical needs:

**Interactive Reports:**
Web-based reports with live data connections, interactive filters, drill-down capabilities, and real-time collaboration. These are the default report type and are best for ongoing analysis and exploration.

**Paginated Reports:**
Pixel-perfect, print-ready reports with fixed layouts, headers, footers, page numbers, and table of contents. Ideal for formal business reports, financial statements, and regulatory filings. Paginated reports can be exported to PDF, Word, or printed directly.

**Embedded Reports:**
Reports designed to be embedded in external applications, websites, or portals. Embedded reports support single sign-on (SSO), custom theming, and parameter passing for seamless integration.

**Scheduled Reports:**
Automatically generated and delivered reports on a recurring schedule. Scheduled reports are rendered as static snapshots (PDF or image) and delivered via email, Slack, or webhook.

### 5.2 Building a Report

To create a new report:

1. Navigate to **Dashboards > + New Dashboard** and select **"Report"** as the type
2. Choose a report template or start from a blank canvas
3. Add content to the report using the report builder toolbar:

**Adding a Data Table:**
```
1. Click "+ Add Component" > "Data Table"
2. Select the data source or query
3. Configure columns:
   - Column Name (display label)
   - Source Field (data field)
   - Format (number, currency, percentage, date)
   - Width (auto, fixed pixels, percentage)
   - Alignment (left, center, right)
   - Conditional Formatting (color rules)
4. Configure table options:
   - Pagination (rows per page)
   - Sorting (default sort column and direction)
   - Totals Row (sum, average, count)
   - Row Grouping (group by field with subtotals)
   - Frozen Columns (keep columns visible during horizontal scroll)
```

**Adding Calculated Fields:**
DataFlow Pro supports calculated fields using a formula language:

```
Revenue Growth = ([Current Period Revenue] - [Prior Period Revenue]) / [Prior Period Revenue]

Customer Lifetime Value = [Average Order Value] * [Purchase Frequency] * [Average Customer Lifespan]

Churn Rate = COUNTIF([Status] = "Churned") / COUNT([Customer ID])

Moving Average = WINDOW_AVG([Revenue], -6, 0, ORDER BY [Month])

YoY Comparison = [Revenue] - LAG([Revenue], 12, ORDER BY [Month])
```

### 5.3 Formatting and Styling

DataFlow Pro provides comprehensive formatting options to ensure reports are professional and on-brand:

**Theme Configuration:**
```json
{
  "theme": {
    "name": "Corporate Blue",
    "colors": {
      "primary": "#1a73e8",
      "secondary": "#34a853",
      "accent": "#fbbc04",
      "background": "#ffffff",
      "surface": "#f8f9fa",
      "text_primary": "#202124",
      "text_secondary": "#5f6368"
    },
    "fonts": {
      "title": "Inter, sans-serif",
      "body": "Roboto, sans-serif",
      "monospace": "Roboto Mono, monospace"
    },
    "chart_palette": [
      "#1a73e8", "#34a853", "#fbbc04", "#ea4335",
      "#46bdc6", "#7b1fa2", "#ff6d00", "#0d47a1"
    ],
    "borders": {
      "radius": "8px",
      "color": "#dadce0"
    }
  }
}
```

**Number Formatting:**

| Format | Example Input | Example Output |
|--------|--------------|----------------|
| Currency (USD) | 1234567.89 | $1,234,567.89 |
| Currency (EUR) | 1234567.89 | 1.234.567,89 EUR |
| Percentage | 0.4523 | 45.23% |
| Compact | 1234567 | 1.23M |
| Scientific | 0.000042 | 4.2 x 10^-5 |
| Custom | 1234567 | 1,234,567.00 |

### 5.4 Report Parameters

Parameters allow report viewers to customize the data displayed in a report without modifying the report itself:

```yaml
Parameters:
  - name: date_range
    label: "Date Range"
    type: date_range
    default: "last_30_days"
    options:
      - { label: "Last 7 Days", value: "last_7_days" }
      - { label: "Last 30 Days", value: "last_30_days" }
      - { label: "Last 90 Days", value: "last_90_days" }
      - { label: "Year to Date", value: "ytd" }
      - { label: "Custom Range", value: "custom" }

  - name: region
    label: "Region"
    type: multi_select
    default: ["all"]
    source: "SELECT DISTINCT region FROM sales_data ORDER BY region"

  - name: min_revenue
    label: "Minimum Revenue"
    type: number
    default: 0
    validation:
      min: 0
      max: 10000000
```

Parameters can be referenced in queries using the `{{parameter_name}}` syntax:

```sql
SELECT
    region,
    product_category,
    SUM(revenue) as total_revenue,
    COUNT(DISTINCT customer_id) as unique_customers
FROM sales_data
WHERE order_date BETWEEN {{date_range.start}} AND {{date_range.end}}
    AND region IN ({{region}})
    AND revenue >= {{min_revenue}}
GROUP BY region, product_category
ORDER BY total_revenue DESC;
```

---

## 6. User Management and Roles

### 6.1 Role Hierarchy

DataFlow Pro uses a role-based access control (RBAC) system with the following pre-defined roles:

| Role | Description | Typical Users |
|------|-------------|---------------|
| **Organization Admin** | Full access to all features, settings, and data. Can manage users, billing, and security settings. | IT administrators, data team leads |
| **Workspace Admin** | Full access within assigned workspaces. Can manage workspace members and permissions. | Team leads, department heads |
| **Editor** | Can create, edit, and delete dashboards, queries, and pipelines within assigned workspaces. | Data analysts, report builders |
| **Analyst** | Can create queries and personal dashboards. Can view shared dashboards. Cannot modify shared assets. | Business analysts, product managers |
| **Viewer** | Read-only access to shared dashboards and reports. Cannot create or modify any assets. | Executives, stakeholders, external partners |

### 6.2 Managing Users

**Adding Users:**
1. Navigate to **Settings > Users**
2. Click **"+ Invite Users"**
3. Enter email addresses (one per line or comma-separated)
4. Select the role to assign
5. Select the workspaces to grant access to
6. Customize the invitation email (optional)
7. Click **"Send Invitations"**

**Bulk User Management:**
For organizations with many users, DataFlow Pro supports bulk operations via CSV import:

```csv
email,first_name,last_name,role,workspaces,teams
jane.doe@company.com,Jane,Doe,editor,"Sales Analytics,Marketing",Analytics Team
john.smith@company.com,John,Smith,viewer,"Executive Dashboards",Leadership
alice.wong@company.com,Alice,Wong,analyst,"Product Analytics",Product Team
```

Upload the CSV file in **Settings > Users > Bulk Import**.

**User Provisioning via SSO/SCIM:**
DataFlow Pro supports automatic user provisioning through SCIM 2.0 protocol integration with identity providers:

- Okta
- Azure Active Directory / Entra ID
- OneLogin
- JumpCloud
- Google Workspace

When SCIM is configured, users are automatically created, updated, and deactivated based on changes in the identity provider. This eliminates the need for manual user management and ensures that access is always in sync with the organization's directory.

### 6.3 Teams and Groups

Teams allow you to organize users into logical groups for easier permission management:

1. Navigate to **Settings > Teams**
2. Click **"+ New Team"**
3. Enter the team name and description
4. Add members to the team
5. Assign workspace access and roles at the team level

Team permissions are inherited by all members. Individual user permissions can override team permissions where a higher level of access is needed.

### 6.4 Workspace Management

Workspaces are isolated environments for organizing data assets. Each workspace has its own dashboards, queries, data connections, and permission settings.

**Creating a Workspace:**
1. Navigate to **Settings > Workspaces**
2. Click **"+ New Workspace"**
3. Enter the workspace name and description
4. Configure workspace settings:
   - Default data source connections
   - Storage quota
   - Allowed export formats
   - Data retention policy
5. Add members or teams with appropriate roles
6. Click **"Create Workspace"**

**Workspace Isolation:**
- Users can only see and access workspaces they are members of
- Data connections in one workspace are not visible to other workspaces (unless explicitly shared)
- Queries and dashboards are workspace-scoped by default
- Cross-workspace sharing requires explicit permissions

### 6.5 API Keys and Service Accounts

For programmatic access and integrations, DataFlow Pro supports API keys and service accounts:

**Personal API Keys:**
1. Navigate to **Profile > API Keys**
2. Click **"+ Generate New Key"**
3. Enter a descriptive name for the key
4. Select the permission scope (read-only, read-write, or admin)
5. Set an expiration date (maximum 1 year)
6. Click **"Generate"**
7. Copy the API key immediately — it will not be shown again

**Service Accounts:**
Service accounts are non-human accounts used for automated processes:

1. Navigate to **Settings > Service Accounts** (admin only)
2. Click **"+ New Service Account"**
3. Enter a name and description
4. Select the role and workspace access
5. Generate API credentials
6. Configure IP allowlist (optional but recommended)

### 6.6 Audit Logging

DataFlow Pro maintains a comprehensive audit log of all user actions for security and compliance purposes. The audit log captures:

- User authentication events (login, logout, failed attempts)
- Data access events (queries executed, exports performed)
- Configuration changes (user management, permission changes, connection updates)
- Administrative actions (billing changes, security settings)

To view the audit log:
1. Navigate to **Settings > Audit Log** (admin only)
2. Filter by user, action type, date range, or resource
3. Export the log for external analysis or compliance reporting

Audit logs are retained for 2 years and cannot be modified or deleted.

---

## 7. API Integration Guide

### 7.1 API Overview

The DataFlow Pro API provides programmatic access to all platform features. The API follows REST conventions and uses JSON for request and response bodies.

**Base URL:** `https://api.dataflowpro.com/v1`

**Authentication:** All API requests must include a valid API key in the Authorization header:

```
Authorization: Bearer YOUR_API_TOKEN
```

**Rate Limits:**

| Plan | Requests per Minute | Requests per Day |
|------|-------------------|-----------------|
| Standard | 60 | 10,000 |
| Professional | 300 | 100,000 |
| Enterprise | 1,000 | Unlimited |

Rate limit headers are included in every response:
```
X-RateLimit-Limit: 300
X-RateLimit-Remaining: 297
X-RateLimit-Reset: 1711234567
```

### 7.2 Authentication Endpoints

**Login (OAuth 2.0):**
```http
POST /auth/token
Content-Type: application/x-www-form-urlencoded

grant_type=client_credentials
&client_id=YOUR_CLIENT_ID
&client_secret=YOUR_CLIENT_SECRET
&scope=read write
```

Response:
```json
{
  "access_token": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "refresh_token": "dGhpcyBpcyBhIHJlZnJlc2ggdG9rZW4...",
  "scope": "read write"
}
```

### 7.3 Data Source Endpoints

**List Data Sources:**
```http
GET /datasources
```

Response:
```json
{
  "data": [
    {
      "id": "ds_abc123",
      "name": "Production Database",
      "type": "postgresql",
      "status": "connected",
      "created_at": "2025-06-15T10:30:00Z",
      "last_sync_at": "2026-03-20T08:00:00Z",
      "tables_count": 42,
      "workspace_id": "ws_xyz789"
    }
  ],
  "pagination": {
    "page": 1,
    "per_page": 20,
    "total": 5,
    "total_pages": 1
  }
}
```

**Create Data Source:**
```http
POST /datasources
Content-Type: application/json

{
  "name": "Analytics Database",
  "type": "postgresql",
  "workspace_id": "ws_xyz789",
  "config": {
    "host": "analytics-db.company.com",
    "port": 5432,
    "database": "analytics",
    "schema": "public",
    "username": "dataflow_user",
    "password": "encrypted_password_here",
    "ssl_mode": "require"
  },
  "sync_schedule": {
    "frequency": "hourly",
    "tables": ["orders", "customers", "products"]
  }
}
```

### 7.4 Query Endpoints

**Execute a Query:**
```http
POST /queries/execute
Content-Type: application/json

{
  "datasource_id": "ds_abc123",
  "sql": "SELECT * FROM orders WHERE created_at >= '2025-01-01' LIMIT 100",
  "timeout": 30,
  "parameters": {
    "start_date": "2025-01-01"
  }
}
```

Response:
```json
{
  "query_id": "qry_exec_456",
  "status": "completed",
  "execution_time_ms": 234,
  "rows_returned": 100,
  "columns": [
    { "name": "order_id", "type": "integer" },
    { "name": "customer_id", "type": "integer" },
    { "name": "total", "type": "decimal" },
    { "name": "created_at", "type": "timestamp" }
  ],
  "data": [
    [1001, 501, 129.99, "2025-01-15T14:30:00Z"],
    [1002, 502, 249.50, "2025-01-15T15:45:00Z"]
  ],
  "truncated": true,
  "total_rows": 45230
}
```

### 7.5 Dashboard Endpoints

**List Dashboards:**
```http
GET /dashboards?workspace_id=ws_xyz789&page=1&per_page=20
```

**Get Dashboard Details:**
```http
GET /dashboards/{dashboard_id}
```

**Create Dashboard:**
```http
POST /dashboards
Content-Type: application/json

{
  "name": "Monthly Sales Dashboard",
  "description": "Key sales metrics and trends",
  "workspace_id": "ws_xyz789",
  "layout": {
    "columns": 24,
    "tiles": [
      {
        "id": "tile_1",
        "type": "chart",
        "position": { "x": 0, "y": 0, "w": 12, "h": 8 },
        "config": {
          "query_id": "qry_monthly_revenue",
          "chart_type": "line",
          "x_axis": "month",
          "y_axis": ["revenue", "target"]
        }
      }
    ]
  }
}
```

### 7.6 Webhook Integration

DataFlow Pro can send webhook notifications for various events:

**Configuring Webhooks:**
1. Navigate to **Settings > Integrations > Webhooks**
2. Click **"+ New Webhook"**
3. Configure the webhook:

```json
{
  "name": "Pipeline Alert Webhook",
  "url": "https://api.company.com/webhooks/dataflow",
  "events": [
    "pipeline.completed",
    "pipeline.failed",
    "alert.triggered",
    "export.completed"
  ],
  "headers": {
    "X-Webhook-Secret": "your_webhook_secret"
  },
  "retry_policy": {
    "max_retries": 3,
    "backoff_seconds": [10, 30, 60]
  }
}
```

**Webhook Payload Example:**
```json
{
  "event": "pipeline.failed",
  "timestamp": "2026-03-20T14:30:00Z",
  "data": {
    "pipeline_id": "pip_abc123",
    "pipeline_name": "Daily Sales ETL",
    "error": "Connection timeout: unable to reach source database",
    "run_id": "run_xyz789",
    "duration_seconds": 300,
    "records_processed": 45000,
    "records_failed": 0
  }
}
```

### 7.7 SDK Libraries

DataFlow Pro provides official SDK libraries for popular programming languages:

**Python:**
```python
from dataflow_pro import DataFlowClient

client = DataFlowClient(api_key="YOUR_API_TOKEN")

# Execute a query
result = client.queries.execute(
    datasource_id="ds_abc123",
    sql="SELECT * FROM orders WHERE total > 100",
    timeout=30
)

# Iterate over results
for row in result.rows:
    print(f"Order {row['order_id']}: ${row['total']}")

# Create a dashboard
dashboard = client.dashboards.create(
    name="My Dashboard",
    workspace_id="ws_xyz789"
)
```

**JavaScript/TypeScript:**
```typescript
import { DataFlowClient } from '@dataflow-pro/sdk';

const client = new DataFlowClient({ apiKey: 'YOUR_API_TOKEN' });

// Execute a query
const result = await client.queries.execute({
  datasourceId: 'ds_abc123',
  sql: 'SELECT * FROM orders WHERE total > 100',
  timeout: 30,
});

// Stream large results
const stream = client.queries.stream({
  datasourceId: 'ds_abc123',
  sql: 'SELECT * FROM large_table',
});

for await (const batch of stream) {
  console.log(`Received ${batch.rows.length} rows`);
}
```

---

## 8. Workflows and Automation

### 8.1 Pipeline Builder

The Pipeline Builder is DataFlow Pro's visual tool for creating data transformation workflows. Pipelines are composed of nodes that represent individual transformation steps, connected by edges that define the data flow.

**Node Types:**

| Node Type | Description | Example Use Cases |
|-----------|-------------|-------------------|
| Source | Reads data from a connected data source | Database table, API endpoint, file |
| Filter | Removes rows based on conditions | Remove nulls, filter by date range |
| Transform | Modifies data values | Type casting, string manipulation, calculations |
| Aggregate | Groups and summarizes data | SUM, COUNT, AVG by category |
| Join | Combines data from multiple sources | Inner, left, right, full outer, cross joins |
| Union | Stacks data from multiple sources | Combining monthly files |
| Sort | Orders data by specified fields | Sort by date descending |
| Deduplicate | Removes duplicate rows | Deduplicate by customer email |
| Pivot | Transforms rows to columns | Monthly values to separate columns |
| Unpivot | Transforms columns to rows | Normalize wide tables |
| Custom SQL | Executes custom SQL transformations | Complex business logic |
| Python | Executes Python scripts | ML model scoring, custom algorithms |
| Destination | Writes data to a target | Database table, file, API |

**Creating a Pipeline:**
1. Navigate to **Pipelines** in the left sidebar
2. Click **"+ New Pipeline"**
3. Drag nodes from the toolbox onto the canvas
4. Connect nodes by dragging from an output port to an input port
5. Configure each node by clicking on it and editing its properties
6. Preview data at any node by clicking **"Preview"** (shows first 100 rows)
7. Click **"Validate"** to check for errors
8. Click **"Save"** and optionally **"Run"** to execute immediately

### 8.2 Scheduling Pipelines

Pipelines can be scheduled to run automatically:

```yaml
Pipeline: Daily Sales ETL
Schedule:
  Frequency: Daily
  Time: 02:00 AM UTC
  Days: Monday through Friday
  Timezone: UTC

Dependencies:
  - pipeline: "Customer Data Sync"
    status: "completed"
  - pipeline: "Product Catalog Update"
    status: "completed"

Retry Policy:
  Max Retries: 3
  Retry Delay: 5 minutes
  Backoff Multiplier: 2

Alerts:
  On Failure:
    - email: data-team@company.com
    - slack: #data-alerts
  On Success (with warnings):
    - email: data-lead@company.com
  On SLA Breach (> 30 minutes):
    - pagerduty: data-oncall
```

### 8.3 Data Alerts

Data alerts monitor your data for specific conditions and notify you when those conditions are met:

**Creating an Alert:**
1. Navigate to **Alerts** in the left sidebar
2. Click **"+ New Alert"**
3. Configure the alert:

```yaml
Alert Name: Revenue Drop Alert
Description: Alert when daily revenue drops more than 20% compared to the 7-day average

Query:
  datasource: ds_abc123
  sql: |
    WITH daily_revenue AS (
      SELECT DATE(order_date) as day, SUM(total) as revenue
      FROM orders
      GROUP BY DATE(order_date)
    ),
    averages AS (
      SELECT
        day,
        revenue,
        AVG(revenue) OVER (ORDER BY day ROWS BETWEEN 7 PRECEDING AND 1 PRECEDING) as avg_7day
      FROM daily_revenue
    )
    SELECT day, revenue, avg_7day,
           (revenue - avg_7day) / avg_7day * 100 as pct_change
    FROM averages
    WHERE day = CURRENT_DATE - 1
    AND (revenue - avg_7day) / avg_7day < -0.20

Condition: Query returns one or more rows

Check Frequency: Daily at 8:00 AM UTC

Notifications:
  - channel: email
    recipients: [sales-leadership@company.com]
    template: revenue_alert
  - channel: slack
    webhook: https://hooks.slack.com/services/xxx
    message: "Revenue Alert: Yesterday's revenue was {{pct_change}}% below the 7-day average"

Cooldown: 24 hours (don't re-alert within this period)
```

### 8.4 Data Quality Rules

DataFlow Pro includes a data quality framework to monitor and enforce data standards:

```yaml
Data Quality Suite: Orders Data Quality
Table: orders
Schedule: After every pipeline run

Rules:
  - name: "No null order IDs"
    type: not_null
    column: order_id
    severity: critical
    action: block_pipeline

  - name: "Valid order status"
    type: accepted_values
    column: status
    values: ["pending", "confirmed", "shipped", "delivered", "cancelled", "returned"]
    severity: warning
    action: log_and_continue

  - name: "Positive order total"
    type: custom_sql
    sql: "SELECT COUNT(*) FROM orders WHERE total <= 0"
    threshold: 0
    severity: critical
    action: block_pipeline

  - name: "Freshness check"
    type: freshness
    column: created_at
    max_age: 24h
    severity: warning
    action: alert

  - name: "Row count anomaly"
    type: volume
    expected_range: [1000, 50000]
    severity: warning
    action: alert

  - name: "Referential integrity"
    type: relationships
    from: orders.customer_id
    to: customers.id
    severity: error
    action: log_and_continue
```

---

## 9. Advanced Configurations

### 9.1 Custom SQL Functions

DataFlow Pro supports creating custom SQL functions that can be reused across queries and pipelines:

```sql
-- Create a custom function for calculating business days between two dates
CREATE FUNCTION business_days_between(start_date DATE, end_date DATE)
RETURNS INTEGER
AS $$
  SELECT COUNT(*)::INTEGER
  FROM generate_series(start_date, end_date, '1 day'::interval) d
  WHERE EXTRACT(DOW FROM d) NOT IN (0, 6)  -- Exclude Saturday and Sunday
$$;

-- Create a custom function for revenue categorization
CREATE FUNCTION revenue_tier(amount DECIMAL)
RETURNS VARCHAR
AS $$
  SELECT CASE
    WHEN amount >= 10000 THEN 'Enterprise'
    WHEN amount >= 1000 THEN 'Mid-Market'
    WHEN amount >= 100 THEN 'SMB'
    ELSE 'Micro'
  END
$$;

-- Usage in queries
SELECT
    customer_name,
    revenue_tier(annual_revenue) as tier,
    business_days_between(contract_start, contract_end) as contract_days
FROM customers;
```

### 9.2 Caching Configuration

DataFlow Pro implements a multi-tier caching system to optimize query performance:

```yaml
Cache Configuration:
  Query Cache:
    Enabled: true
    TTL: 300 seconds (5 minutes)
    Max Size: 10 GB
    Eviction Policy: LRU (Least Recently Used)
    Cache Key: hash(query_text + parameters + user_permissions)

  Dashboard Cache:
    Enabled: true
    TTL: 60 seconds (1 minute)
    Refresh: On dashboard load
    Preload: Enabled for scheduled dashboards

  Metadata Cache:
    Enabled: true
    TTL: 3600 seconds (1 hour)
    Includes: Table schemas, column stats, connection metadata

  Cache Invalidation:
    On Data Change: Automatic (via CDC listeners)
    On Schema Change: Automatic
    Manual: Available via API and UI
```

To configure caching for a specific query:
```sql
-- Cache this query for 1 hour
-- @cache: ttl=3600, key="daily_summary_{date}"
SELECT
    DATE(order_date) as order_day,
    COUNT(*) as order_count,
    SUM(total) as total_revenue
FROM orders
WHERE order_date = CURRENT_DATE
GROUP BY DATE(order_date);
```

### 9.3 Performance Tuning

**Query Optimization:**
DataFlow Pro provides a query analyzer that identifies performance bottlenecks and suggests optimizations:

1. Run your query with the **"Analyze"** option enabled
2. Review the query execution plan
3. Apply suggested optimizations:

| Issue | Suggestion | Impact |
|-------|-----------|--------|
| Full table scan | Add index on filtered columns | High |
| Large result set | Add LIMIT or pagination | Medium |
| Expensive joins | Reorder join sequence; add join hints | High |
| Repeated subqueries | Use CTEs or materialized views | Medium |
| Unnecessary columns | Select only required columns | Low |
| Missing partition pruning | Add partition key to WHERE clause | High |

**Connection Pooling:**
```yaml
Connection Pool Settings:
  Min Connections: 5
  Max Connections: 50
  Idle Timeout: 300 seconds
  Max Lifetime: 3600 seconds
  Validation Query: "SELECT 1"
  Validation Interval: 30 seconds
```

### 9.4 Custom Connectors

For data sources not supported natively, DataFlow Pro provides a Custom Connector SDK:

```python
from dataflow_pro.connectors import BaseConnector, ConnectorConfig, Schema

class CustomAPIConnector(BaseConnector):
    """Custom connector for the Acme Internal API."""

    name = "acme_internal_api"
    display_name = "Acme Internal API"
    description = "Connects to Acme's internal REST API"

    config_schema = ConnectorConfig(
        fields=[
            {"name": "base_url", "type": "string", "required": True},
            {"name": "api_key", "type": "secret", "required": True},
            {"name": "timeout", "type": "integer", "default": 30},
        ]
    )

    def test_connection(self) -> bool:
        response = self.http_client.get(
            f"{self.config.base_url}/health",
            headers={"Authorization": f"Bearer {self.config.api_key}"}
        )
        return response.status_code == 200

    def discover_schema(self) -> list[Schema]:
        return [
            Schema(
                name="employees",
                columns=[
                    {"name": "id", "type": "integer"},
                    {"name": "name", "type": "string"},
                    {"name": "department", "type": "string"},
                    {"name": "hire_date", "type": "date"},
                ]
            ),
            Schema(
                name="departments",
                columns=[
                    {"name": "id", "type": "integer"},
                    {"name": "name", "type": "string"},
                    {"name": "head_count", "type": "integer"},
                ]
            )
        ]

    def fetch_data(self, table: str, filters: dict = None) -> Iterator[dict]:
        page = 1
        while True:
            response = self.http_client.get(
                f"{self.config.base_url}/{table}",
                params={"page": page, "per_page": 100, **(filters or {})},
                headers={"Authorization": f"Bearer {self.config.api_key}"}
            )
            data = response.json()
            if not data["results"]:
                break
            yield from data["results"]
            page += 1
```

### 9.5 Environment Configuration

DataFlow Pro supports multiple environments (development, staging, production) with environment-specific configurations:

```yaml
# dataflow-pro.yaml
environments:
  development:
    datasources:
      primary_db:
        host: dev-db.internal.company.com
        database: analytics_dev
    settings:
      query_timeout: 120
      cache_ttl: 60
      log_level: DEBUG
      enable_query_profiling: true

  staging:
    datasources:
      primary_db:
        host: staging-db.internal.company.com
        database: analytics_staging
    settings:
      query_timeout: 60
      cache_ttl: 300
      log_level: INFO
      enable_query_profiling: true

  production:
    datasources:
      primary_db:
        host: prod-db.internal.company.com
        database: analytics_prod
        read_replicas:
          - prod-db-replica-1.internal.company.com
          - prod-db-replica-2.internal.company.com
    settings:
      query_timeout: 30
      cache_ttl: 600
      log_level: WARN
      enable_query_profiling: false
```

---

## 10. Security and Compliance

### 10.1 Security Architecture

DataFlow Pro implements defense-in-depth security with multiple layers of protection:

**Network Security:**
- All data in transit is encrypted using TLS 1.3
- Dedicated VPC isolation for enterprise customers
- IP allowlisting for API and admin access
- DDoS protection via CloudFlare Enterprise
- Web Application Firewall (WAF) with OWASP rule sets

**Application Security:**
- OAuth 2.0 / OpenID Connect authentication
- Role-based access control (RBAC) with fine-grained permissions
- API key rotation and expiration enforcement
- Session management with configurable timeout and concurrent session limits
- CSRF protection on all state-changing operations
- Content Security Policy (CSP) headers

**Data Security:**
- AES-256 encryption for data at rest
- Column-level encryption for sensitive fields (PII, payment data)
- Data masking and anonymization for non-production environments
- Automated PII detection and classification
- Customer-managed encryption keys (CMEK) for enterprise customers

### 10.2 Single Sign-On (SSO)

DataFlow Pro supports enterprise SSO integration via the following protocols:

**SAML 2.0:**
```xml
<!-- DataFlow Pro Service Provider Metadata -->
<EntityDescriptor entityID="https://app.dataflowpro.com/saml/metadata">
  <SPSSODescriptor
    AuthnRequestsSigned="true"
    WantAssertionsSigned="true"
    protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <AssertionConsumerService
      Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST"
      Location="https://app.dataflowpro.com/saml/acs"
      index="0" />
    <SingleLogoutService
      Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect"
      Location="https://app.dataflowpro.com/saml/logout" />
  </SPSSODescriptor>
</EntityDescriptor>
```

**OpenID Connect:**
```json
{
  "issuer": "https://auth.company.com",
  "authorization_endpoint": "https://auth.company.com/authorize",
  "token_endpoint": "https://auth.company.com/token",
  "userinfo_endpoint": "https://auth.company.com/userinfo",
  "client_id": "dataflow-pro-client",
  "client_secret": "YOUR_CLIENT_SECRET",
  "scopes": ["openid", "profile", "email", "groups"],
  "response_type": "code",
  "redirect_uri": "https://app.dataflowpro.com/auth/callback"
}
```

### 10.3 Compliance Certifications

DataFlow Pro maintains the following compliance certifications and attestations:

| Certification | Status | Last Audit | Report Available |
|--------------|--------|------------|-----------------|
| SOC 2 Type II | Active | January 2026 | Upon request (NDA required) |
| ISO 27001 | Active | March 2025 | Upon request |
| ISO 27701 | Active | March 2025 | Upon request |
| GDPR | Compliant | Ongoing | DPA available |
| HIPAA | Compliant (Enterprise plan) | June 2025 | BAA available |
| PCI DSS Level 1 | Active | September 2025 | AOC available |
| CCPA | Compliant | Ongoing | Privacy policy |
| FedRAMP Moderate | In Progress | Expected Q3 2026 | N/A |

### 10.4 Data Residency

DataFlow Pro offers data residency options to meet regulatory and compliance requirements:

| Region | Data Center Location | Available Plans |
|--------|---------------------|-----------------|
| US East | Virginia, USA | All plans |
| US West | Oregon, USA | Professional, Enterprise |
| EU Central | Frankfurt, Germany | Professional, Enterprise |
| EU West | Dublin, Ireland | Enterprise |
| Asia Pacific | Tokyo, Japan | Enterprise |
| Asia Pacific | Sydney, Australia | Enterprise |
| Canada | Montreal, Canada | Enterprise |
| UK | London, UK | Enterprise |

Enterprise customers can request additional data residency options. All data processing occurs within the selected region, and no data is transferred to other regions without explicit customer consent.

### 10.5 Vulnerability Management

DataFlow Pro maintains a comprehensive vulnerability management program:

- Automated vulnerability scanning of all infrastructure and application components (daily)
- Third-party penetration testing conducted quarterly by independent security firms
- Bug bounty program for responsible disclosure of security vulnerabilities
- Patch management SLA: critical vulnerabilities patched within 24 hours, high within 7 days
- Dependency scanning and software composition analysis (SCA) in CI/CD pipeline
- Container image scanning before deployment

To report a security vulnerability, contact security@dataflowpro.com. DataFlow Pro follows responsible disclosure practices and does not pursue legal action against security researchers who report vulnerabilities in good faith.

---

## 11. Troubleshooting Common Issues

### 11.1 Connection Issues

**Problem: Cannot connect to database**
```
Error: Connection refused: unable to connect to host:port
```

**Solution:**
1. Verify the hostname and port are correct
2. Check that the database server is running and accepting connections
3. Verify network connectivity (firewall rules, security groups, VPN)
4. If using SSH tunnel, verify SSH credentials and that the bastion host is accessible
5. Check that the DataFlow Pro IP addresses are allowlisted:
   - US East: 52.10.20.30/32, 52.10.20.31/32
   - EU Central: 18.195.20.30/32, 18.195.20.31/32
6. Test the connection from a different network to isolate the issue

**Problem: Authentication failed**
```
Error: FATAL: password authentication failed for user "dataflow_user"
```

**Solution:**
1. Verify the username and password are correct
2. Check that the user has been granted access to the specified database and schema
3. For PostgreSQL, check `pg_hba.conf` for IP-based access rules
4. For MySQL, verify the user is allowed to connect from the DataFlow Pro IP addresses
5. Reset the password and update the connection in DataFlow Pro

### 11.2 Query Performance Issues

**Problem: Query is running slowly**

**Diagnostic Steps:**
1. Click **"Analyze"** on the query to view the execution plan
2. Check for the following common issues:

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| Sequential scan on large table | Missing index | Create index on filtered/joined columns |
| High memory usage | Large intermediate results | Add filters early in the query; use LIMIT |
| Slow joins | Joining on non-indexed columns | Add indexes; restructure join order |
| Timeout | Query exceeds configured timeout | Optimize query or increase timeout setting |
| Lock contention | Concurrent queries on same tables | Schedule queries to avoid conflicts |

3. Consider materializing frequently-used subqueries:
```sql
-- Create a materialized view for expensive aggregations
CREATE MATERIALIZED VIEW monthly_sales AS
SELECT
    DATE_TRUNC('month', order_date) as month,
    product_category,
    SUM(revenue) as total_revenue,
    COUNT(DISTINCT customer_id) as unique_customers
FROM orders
GROUP BY 1, 2;

-- Refresh on schedule
REFRESH MATERIALIZED VIEW monthly_sales;
```

### 11.3 Dashboard Loading Issues

**Problem: Dashboard is slow to load**

**Solution:**
1. Check the number of tiles on the dashboard. Dashboards with more than 20 tiles may experience slower load times. Consider splitting into multiple dashboards or using tab navigation.
2. Review individual chart query performance using the Dashboard Performance Monitor (**Dashboard Settings > Performance**)
3. Enable caching for dashboards that do not require real-time data
4. Reduce the date range or add default filters to limit the amount of data loaded initially
5. Use lazy loading for below-the-fold tiles (**Dashboard Settings > Loading > Lazy Loading**)

**Problem: Charts not rendering correctly**

**Solution:**
1. Clear browser cache and reload the page (Ctrl+Shift+R / Cmd+Shift+R)
2. Check that your browser is supported and up to date
3. Disable browser extensions that may interfere with rendering (ad blockers, privacy extensions)
4. Enable hardware acceleration in browser settings
5. If the issue persists, export the dashboard configuration and contact support

### 11.4 Data Import Issues

**Problem: CSV import fails with parsing errors**
```
Error: Line 1523: Expected 12 fields, found 13. Possible unescaped delimiter in field value.
```

**Solution:**
1. Open the CSV file and check line 1523 for data issues
2. Common causes:
   - Field values containing the delimiter character (comma in field text)
   - Mismatched or missing quote characters
   - Line breaks within quoted field values
   - Incorrect encoding (e.g., file is UTF-16 but configured as UTF-8)
3. Configure the import to use a different delimiter or quote character
4. Enable the **"Flexible Parsing"** option to handle minor formatting issues
5. Use the **"Error Log"** to identify and fix all problematic rows

**Problem: Data types not detected correctly**

**Solution:**
1. Review the auto-detected types in the import preview
2. Override incorrect types manually:
   - Dates detected as strings: specify the date format pattern
   - Numbers detected as strings: check for currency symbols, commas, or spaces
   - Integers detected as floats: may have null values represented as empty strings
3. Create a schema definition file for recurring imports to ensure consistent type detection

### 11.5 Authentication and Access Issues

**Problem: SSO login fails**
```
Error: SAML Response validation failed: Audience mismatch
```

**Solution:**
1. Verify the Entity ID in your Identity Provider matches the DataFlow Pro SP Entity ID
2. Check that the ACS (Assertion Consumer Service) URL is correctly configured
3. Ensure the SAML Response is signed and the certificate matches
4. Check clock skew between the IdP and DataFlow Pro (must be within 5 minutes)
5. Review the SAML Response in the browser's developer tools (Network tab) for detailed error information

**Problem: User cannot access a specific dashboard**

**Solution:**
1. Check the user's role and workspace membership (**Settings > Users**)
2. Verify the dashboard permissions (**Dashboard Settings > Sharing**)
3. Check if the user's team has access to the dashboard's workspace
4. If using row-level security, verify the user's security context matches the dashboard's data
5. Check the audit log for any recent permission changes

### 11.6 API Issues

**Problem: API returns 429 Too Many Requests**

**Solution:**
1. Check the rate limit headers in the response to determine your current usage
2. Implement exponential backoff in your API client
3. Batch multiple operations into fewer API calls where possible
4. Consider upgrading your plan for higher rate limits
5. Use webhooks instead of polling for event-driven workflows

```python
import time
import requests

def api_call_with_retry(url, headers, max_retries=5):
    for attempt in range(max_retries):
        response = requests.get(url, headers=headers)
        if response.status_code == 429:
            retry_after = int(response.headers.get('Retry-After', 60))
            wait_time = min(retry_after, 2 ** attempt * 10)
            time.sleep(wait_time)
            continue
        return response
    raise Exception("Max retries exceeded")
```

---

## 12. Appendices

### Appendix A: Keyboard Shortcuts

| Action | Windows/Linux | macOS |
|--------|--------------|-------|
| Run query | Ctrl + Enter | Cmd + Enter |
| Save | Ctrl + S | Cmd + S |
| New query tab | Ctrl + T | Cmd + T |
| Close tab | Ctrl + W | Cmd + W |
| Toggle sidebar | Ctrl + B | Cmd + B |
| Global search | Ctrl + K | Cmd + K |
| Format SQL | Ctrl + Shift + F | Cmd + Shift + F |
| Comment line | Ctrl + / | Cmd + / |
| Autocomplete | Ctrl + Space | Ctrl + Space |
| Undo | Ctrl + Z | Cmd + Z |
| Redo | Ctrl + Shift + Z | Cmd + Shift + Z |
| Full screen | F11 | Ctrl + Cmd + F |
| Toggle dark mode | Ctrl + Shift + D | Cmd + Shift + D |

### Appendix B: SQL Function Reference

**String Functions:**

| Function | Description | Example |
|----------|-------------|---------|
| `CONCAT(a, b)` | Concatenate strings | `CONCAT('Hello', ' World')` = `'Hello World'` |
| `UPPER(s)` | Convert to uppercase | `UPPER('hello')` = `'HELLO'` |
| `LOWER(s)` | Convert to lowercase | `LOWER('HELLO')` = `'hello'` |
| `TRIM(s)` | Remove whitespace | `TRIM('  hello  ')` = `'hello'` |
| `LENGTH(s)` | String length | `LENGTH('hello')` = `5` |
| `SUBSTRING(s, start, len)` | Extract substring | `SUBSTRING('hello', 1, 3)` = `'hel'` |
| `REPLACE(s, old, new)` | Replace occurrences | `REPLACE('hello', 'l', 'r')` = `'herro'` |
| `REGEXP_EXTRACT(s, pattern)` | Extract regex match | `REGEXP_EXTRACT('abc123', '[0-9]+')` = `'123'` |
| `SPLIT(s, delimiter)` | Split into array | `SPLIT('a,b,c', ',')` = `['a','b','c']` |

**Date Functions:**

| Function | Description | Example |
|----------|-------------|---------|
| `CURRENT_DATE` | Today's date | `2026-03-20` |
| `CURRENT_TIMESTAMP` | Current timestamp | `2026-03-20 14:30:00` |
| `DATE_TRUNC(part, date)` | Truncate to precision | `DATE_TRUNC('month', '2026-03-20')` = `'2026-03-01'` |
| `DATE_ADD(date, interval)` | Add interval | `DATE_ADD('2026-03-20', INTERVAL 7 DAY)` = `'2026-03-27'` |
| `DATE_DIFF(a, b)` | Days between dates | `DATE_DIFF('2026-03-20', '2026-03-01')` = `19` |
| `EXTRACT(part FROM date)` | Extract component | `EXTRACT(MONTH FROM '2026-03-20')` = `3` |
| `FORMAT_DATE(date, fmt)` | Format as string | `FORMAT_DATE('2026-03-20', 'MMM DD, YYYY')` = `'Mar 20, 2026'` |

**Aggregate Functions:**

| Function | Description |
|----------|-------------|
| `COUNT(*)` | Count all rows |
| `COUNT(DISTINCT col)` | Count distinct values |
| `SUM(col)` | Sum of values |
| `AVG(col)` | Average value |
| `MIN(col)` | Minimum value |
| `MAX(col)` | Maximum value |
| `MEDIAN(col)` | Median value |
| `PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY col)` | 95th percentile |
| `STDDEV(col)` | Standard deviation |
| `VARIANCE(col)` | Variance |
| `ARRAY_AGG(col)` | Collect into array |
| `STRING_AGG(col, delimiter)` | Concatenate with delimiter |

### Appendix C: Error Code Reference

| Error Code | Description | Resolution |
|-----------|-------------|------------|
| DFP-1001 | Authentication failed | Check credentials; reset password if needed |
| DFP-1002 | Insufficient permissions | Contact workspace admin for access |
| DFP-1003 | Session expired | Re-authenticate |
| DFP-2001 | Data source connection failed | Check connection settings and network access |
| DFP-2002 | Query syntax error | Review SQL syntax; check column names |
| DFP-2003 | Query timeout | Optimize query or increase timeout setting |
| DFP-2004 | Result set too large | Add LIMIT clause or filters |
| DFP-3001 | Import parsing error | Check file format and encoding |
| DFP-3002 | Schema mismatch | Verify column types match destination table |
| DFP-3003 | Duplicate key violation | Check for duplicate primary key values |
| DFP-4001 | Pipeline execution failed | Check pipeline logs for detailed error |
| DFP-4002 | Pipeline dependency not met | Ensure dependent pipelines completed successfully |
| DFP-5001 | Rate limit exceeded | Reduce request frequency; implement backoff |
| DFP-5002 | Quota exceeded | Upgrade plan or contact sales |
| DFP-6001 | Export failed | Check export configuration and destination access |
| DFP-6002 | Scheduled delivery failed | Verify recipient email/Slack channel configuration |

### Appendix D: Glossary

| Term | Definition |
|------|------------|
| **CDC** | Change Data Capture — a method of tracking changes in a source database for incremental synchronization |
| **CTE** | Common Table Expression — a temporary named result set defined within a SQL query using the WITH clause |
| **DAG** | Directed Acyclic Graph — the structure used to represent pipeline dependencies and execution order |
| **ELT** | Extract, Load, Transform — a data integration pattern where data is loaded before transformation |
| **ETL** | Extract, Transform, Load — a data integration pattern where data is transformed before loading |
| **RBAC** | Role-Based Access Control — a method of restricting system access based on user roles |
| **RLS** | Row-Level Security — a security feature that restricts which rows a user can see based on their attributes |
| **SCIM** | System for Cross-domain Identity Management — a standard protocol for automating user provisioning |
| **SSO** | Single Sign-On — an authentication method that allows users to access multiple applications with one set of credentials |
| **VPC** | Virtual Private Cloud — an isolated network environment within a cloud provider |

---

*DataFlow Pro is a product of DataFlow Systems, Inc. All rights reserved. This documentation is provided for informational purposes and is subject to change without notice. For the latest documentation, visit https://docs.dataflowpro.com.*

*Support: support@dataflowpro.com | Status: status.dataflowpro.com | Community: community.dataflowpro.com*
