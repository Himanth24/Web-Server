// Initialize counter variables
let count = 0;
const countDisplay = document.getElementById('count');
const incrementBtn = document.getElementById('increment');
const decrementBtn = document.getElementById('decrement');
const resetBtn = document.getElementById('reset');

// Update the display with current count
function updateDisplay() {
    countDisplay.textContent = count;
    
    // Apply color based on count value
    if (count > 0) {
        countDisplay.className = 'count-display positive';
    } else if (count < 0) {
        countDisplay.className = 'count-display negative';
    } else {
        countDisplay.className = 'count-display neutral';
    }
}

// Event listeners for buttons
incrementBtn.addEventListener('click', () => {
    count++;
    updateDisplay();
    animateButton(incrementBtn);
});

decrementBtn.addEventListener('click', () => {
    count--;
    updateDisplay();
    animateButton(decrementBtn);
});

resetBtn.addEventListener('click', () => {
    count = 0;
    updateDisplay();
    animateButton(resetBtn);
});

// Button click animation
function animateButton(button) {
    button.style.transform = 'scale(0.95)';
    setTimeout(() => {
        button.style.transform = '';
    }, 100);
}