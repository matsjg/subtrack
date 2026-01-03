// API Base URL
const API_BASE = '/api';

// State
let subscriptions = [];
let summary = null;
let currentEditId = null;

// Initialize app
document.addEventListener('DOMContentLoaded', () => {
    loadData();
    setupEventListeners();
});

// Event Listeners
function setupEventListeners() {
    // Add subscription button
    document.getElementById('add-btn').addEventListener('click', () => {
        openModal();
    });

    // Modal close buttons
    document.getElementById('close-modal').addEventListener('click', closeModal);
    document.getElementById('cancel-btn').addEventListener('click', closeModal);

    // Form submission
    document.getElementById('subscription-form').addEventListener('submit', handleFormSubmit);

    // Import/Export
    document.getElementById('import-btn').addEventListener('click', openImportModal);
    document.getElementById('export-btn').addEventListener('click', exportCSV);
    document.getElementById('close-import-modal').addEventListener('click', closeImportModal);
    document.getElementById('cancel-import-btn').addEventListener('click', closeImportModal);
    document.getElementById('csv-file').addEventListener('change', handleFileSelect);
    document.getElementById('upload-btn').addEventListener('click', handleImport);

    // Close modals on background click
    document.getElementById('subscription-modal').addEventListener('click', (e) => {
        if (e.target.id === 'subscription-modal') closeModal();
    });
    document.getElementById('import-modal').addEventListener('click', (e) => {
        if (e.target.id === 'import-modal') closeImportModal();
    });
}

// Data Loading
async function loadData() {
    try {
        await Promise.all([loadSubscriptions(), loadSummary()]);
    } catch (error) {
        showToast('Failed to load data', 'error');
        console.error(error);
    }
}

async function loadSubscriptions() {
    const response = await fetch(`${API_BASE}/subscriptions`);
    if (!response.ok) throw new Error('Failed to load subscriptions');
    subscriptions = await response.json();
    renderSubscriptions();
}

async function loadSummary() {
    const response = await fetch(`${API_BASE}/summary`);
    if (!response.ok) throw new Error('Failed to load summary');
    summary = await response.json();
    renderSummary();
}

// Rendering
function renderSummary() {
    if (!summary) return;

    document.getElementById('monthly-total').textContent = `$${summary.total_monthly.toFixed(2)}`;
    document.getElementById('yearly-total').textContent = `$${summary.total_yearly.toFixed(2)}`;
    document.getElementById('active-count').textContent = summary.active_count;

    renderUpcomingRenewals();
    renderCategories();
}

function renderSubscriptions() {
    const container = document.getElementById('subscriptions-list');

    if (subscriptions.length === 0) {
        container.innerHTML = `
            <div class="empty-state">
                <p>No subscriptions yet</p>
                <p style="font-size: 0.875rem;">Click "Add Subscription" to get started</p>
            </div>
        `;
        return;
    }

    container.innerHTML = subscriptions.map(sub => {
        const isUpcoming = summary?.upcoming_renewals?.some(r => r.id === sub.id);
        const categoryDisplay = sub.category ? `<span>${sub.category}</span>` : '';

        return `
            <div class="subscription-item ${isUpcoming ? 'upcoming' : ''} ${sub.status === 'canceled' ? 'canceled' : ''}">
                <div class="subscription-info">
                    <div class="subscription-header">
                        <span class="subscription-name">${escapeHtml(sub.name)}</span>
                        <span class="badge ${sub.status}">${sub.status}</span>
                    </div>
                    <div class="subscription-meta">
                        <span><strong>$${sub.cost.toFixed(2)}</strong> / ${sub.billing_cycle}</span>
                        <span>Renews: ${formatDate(sub.renewal_date)}</span>
                        ${categoryDisplay}
                    </div>
                </div>
                <div class="subscription-cost">
                    $${(sub.cost / getBillingCycleMonths(sub.billing_cycle)).toFixed(2)}/mo
                </div>
                <div class="subscription-actions">
                    ${sub.status === 'active' ? `
                        <button class="btn btn-sm btn-secondary" onclick="editSubscription(${sub.id})">Edit</button>
                        <button class="btn btn-sm btn-secondary" onclick="cancelSubscription(${sub.id})">Cancel</button>
                    ` : ''}
                    ${sub.status === 'canceled' ? `
                        <button class="btn btn-sm btn-success" onclick="reactivateSubscription(${sub.id})">Reactivate</button>
                    ` : ''}
                    <button class="btn btn-sm btn-danger" onclick="deleteSubscription(${sub.id})">Delete</button>
                </div>
            </div>
        `;
    }).join('');
}

function renderUpcomingRenewals() {
    const container = document.getElementById('upcoming-renewals');

    if (!summary?.upcoming_renewals || summary.upcoming_renewals.length === 0) {
        container.innerHTML = '<p class="empty-state" style="padding: 1rem;">No upcoming renewals</p>';
        return;
    }

    container.innerHTML = summary.upcoming_renewals.map(renewal => `
        <div class="renewal-item">
            <div class="renewal-header">
                <span class="renewal-name">${escapeHtml(renewal.name)}</span>
                <span class="renewal-days">${renewal.days_until}d</span>
            </div>
            <div class="renewal-date">
                ${formatDate(renewal.next_renewal)} - $${renewal.cost.toFixed(2)}
            </div>
        </div>
    `).join('');
}

function renderCategories() {
    const container = document.getElementById('category-breakdown');

    if (!summary?.categories || Object.keys(summary.categories).length === 0) {
        container.innerHTML = '<p class="empty-state" style="padding: 1rem;">No categories</p>';
        return;
    }

    container.innerHTML = Object.entries(summary.categories)
        .sort((a, b) => b[1].monthly - a[1].monthly)
        .map(([name, data]) => `
            <div class="category-item">
                <div class="category-header">
                    <span class="category-name">${escapeHtml(name)}</span>
                </div>
                <div class="category-stats">
                    $${data.monthly.toFixed(2)}/mo · ${data.count} subscription${data.count !== 1 ? 's' : ''}
                </div>
            </div>
        `).join('');

    // Update category suggestions
    const datalist = document.getElementById('category-suggestions');
    datalist.innerHTML = Object.keys(summary.categories)
        .map(cat => `<option value="${escapeHtml(cat)}">`)
        .join('');
}

// Modal Management
function openModal(subscription = null) {
    const modal = document.getElementById('subscription-modal');
    const form = document.getElementById('subscription-form');
    const title = document.getElementById('modal-title');

    form.reset();
    currentEditId = null;

    if (subscription) {
        title.textContent = 'Edit Subscription';
        currentEditId = subscription.id;
        document.getElementById('subscription-id').value = subscription.id;
        document.getElementById('name').value = subscription.name;
        document.getElementById('cost').value = subscription.cost;
        document.getElementById('billing-cycle').value = subscription.billing_cycle;
        document.getElementById('renewal-date').value = subscription.renewal_date;
        document.getElementById('category').value = subscription.category || '';
        document.getElementById('notes').value = subscription.notes || '';
        document.getElementById('reminder-days').value = subscription.reminder_days;
    } else {
        title.textContent = 'Add Subscription';
        // Set default renewal date to today
        const today = new Date().toISOString().split('T')[0];
        document.getElementById('renewal-date').value = today;
    }

    modal.classList.add('active');
}

function closeModal() {
    document.getElementById('subscription-modal').classList.remove('active');
    currentEditId = null;
}

function openImportModal() {
    document.getElementById('import-modal').classList.add('active');
    document.getElementById('import-results').classList.add('hidden');
    document.getElementById('csv-file').value = '';
    document.getElementById('file-name').textContent = 'Choose a CSV file';
    document.getElementById('upload-btn').disabled = true;
}

function closeImportModal() {
    document.getElementById('import-modal').classList.remove('active');
}

// Form Handling
async function handleFormSubmit(e) {
    e.preventDefault();

    const data = {
        name: document.getElementById('name').value,
        cost: parseFloat(document.getElementById('cost').value),
        billing_cycle: document.getElementById('billing-cycle').value,
        renewal_date: document.getElementById('renewal-date').value,
        category: document.getElementById('category').value || null,
        notes: document.getElementById('notes').value || null,
        reminder_days: parseInt(document.getElementById('reminder-days').value),
    };

    try {
        let response;
        if (currentEditId) {
            response = await fetch(`${API_BASE}/subscriptions/${currentEditId}`, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(data),
            });
        } else {
            response = await fetch(`${API_BASE}/subscriptions`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(data),
            });
        }

        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.error || 'Failed to save subscription');
        }

        showToast(currentEditId ? 'Subscription updated' : 'Subscription created', 'success');
        closeModal();
        loadData();
    } catch (error) {
        showToast(error.message, 'error');
    }
}

// Subscription Actions
async function editSubscription(id) {
    const subscription = subscriptions.find(s => s.id === id);
    if (subscription) {
        openModal(subscription);
    }
}

async function deleteSubscription(id) {
    if (!confirm('Are you sure you want to delete this subscription?')) return;

    try {
        const response = await fetch(`${API_BASE}/subscriptions/${id}`, {
            method: 'DELETE',
        });

        if (!response.ok) throw new Error('Failed to delete subscription');

        showToast('Subscription deleted', 'success');
        loadData();
    } catch (error) {
        showToast(error.message, 'error');
    }
}

async function cancelSubscription(id) {
    try {
        const response = await fetch(`${API_BASE}/subscriptions/${id}/cancel`, {
            method: 'POST',
        });

        if (!response.ok) throw new Error('Failed to cancel subscription');

        showToast('Subscription canceled', 'success');
        loadData();
    } catch (error) {
        showToast(error.message, 'error');
    }
}

async function reactivateSubscription(id) {
    try {
        const response = await fetch(`${API_BASE}/subscriptions/${id}/reactivate`, {
            method: 'POST',
        });

        if (!response.ok) throw new Error('Failed to reactivate subscription');

        showToast('Subscription reactivated', 'success');
        loadData();
    } catch (error) {
        showToast(error.message, 'error');
    }
}

// Import/Export
async function exportCSV() {
    try {
        const response = await fetch(`${API_BASE}/export`);
        if (!response.ok) throw new Error('Failed to export');

        const blob = await response.blob();
        const url = window.URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = 'subscriptions.csv';
        document.body.appendChild(a);
        a.click();
        window.URL.revokeObjectURL(url);
        document.body.removeChild(a);

        showToast('Subscriptions exported', 'success');
    } catch (error) {
        showToast(error.message, 'error');
    }
}

function handleFileSelect(e) {
    const file = e.target.files[0];
    if (file) {
        document.getElementById('file-name').textContent = file.name;
        document.getElementById('upload-btn').disabled = false;
    }
}

async function handleImport() {
    const fileInput = document.getElementById('csv-file');
    const file = fileInput.files[0];

    if (!file) return;

    try {
        const text = await file.text();
        const response = await fetch(`${API_BASE}/import`, {
            method: 'POST',
            headers: { 'Content-Type': 'text/csv' },
            body: text,
        });

        if (!response.ok) throw new Error('Failed to import');

        const result = await response.json();
        const resultsDiv = document.getElementById('import-results');
        resultsDiv.classList.remove('hidden');

        if (result.errors.length === 0) {
            resultsDiv.classList.add('success');
            resultsDiv.classList.remove('error');
            resultsDiv.innerHTML = `
                <strong>Success!</strong>
                <p>Imported ${result.imported} subscription${result.imported !== 1 ? 's' : ''}</p>
            `;
            showToast(`Imported ${result.imported} subscriptions`, 'success');
            loadData();
        } else {
            resultsDiv.classList.add('error');
            resultsDiv.classList.remove('success');
            resultsDiv.innerHTML = `
                <strong>Import completed with errors</strong>
                <p>Imported: ${result.imported} subscriptions</p>
                <p>Errors:</p>
                <ul style="margin-left: 1.5rem; margin-top: 0.5rem;">
                    ${result.errors.map(err => `<li>${escapeHtml(err)}</li>`).join('')}
                </ul>
            `;
        }
    } catch (error) {
        showToast(error.message, 'error');
    }
}

// Utilities
function formatDate(dateString) {
    const date = new Date(dateString + 'T00:00:00');
    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
}

function getBillingCycleMonths(cycle) {
    const months = {
        weekly: 1 / 4.33,
        monthly: 1,
        quarterly: 3,
        yearly: 12,
    };
    return months[cycle] || 1;
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function showToast(message, type = 'info') {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    toast.className = `toast ${type}`;
    toast.textContent = message;

    container.appendChild(toast);

    setTimeout(() => {
        toast.remove();
    }, 5000);
}
