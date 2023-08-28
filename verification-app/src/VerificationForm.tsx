// src/VerificationForm.tsx
import axios from 'axios';
import React, { useState, ChangeEvent, FormEvent } from 'react';
import { TextField, Button, FormControlLabel, Checkbox, Container, Typography, CircularProgress } from '@mui/material';

interface FormData {
    rpc_url: string;
    cache?: string;
    network: string;
    block_no: number;
    local_exec?: number;
    submit_to_bonsai: boolean;
    verify_bonsai_receipt_uuid?: string;
}

const VerificationForm: React.FC = () => {
    const [formData, setFormData] = useState<FormData>({
        rpc_url: '',
        network: '',
        block_no: 0,
        submit_to_bonsai: false,
    });

    const [loading, setLoading] = useState(false);
    const [showCache, setShowCache] = useState(false);
    const [showLocalExec, setShowLocalExec] = useState(false);
    const [showReceiptUUID, setShowReceiptUUID] = useState(false);
    const [serverResponse, setServerResponse] = useState<string | null>(null);

    const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
        const value = event.target.type === 'checkbox' ? event.target.checked : event.target.value;
        setFormData({
            ...formData,
            [event.target.name]: (event.target.name === 'block_no' || event.target.name === 'local_exec') && typeof value === 'string' ? parseInt(value) : value
        });
    };

    const handleSubmit = async (event: FormEvent) => {
        event.preventDefault();
        setLoading(true);
        const dataToSend = { ...formData };
        if (!showCache) delete dataToSend.cache;
        if (!showLocalExec) delete dataToSend.local_exec;
        if (!showReceiptUUID) delete dataToSend.verify_bonsai_receipt_uuid;
        try {
            const response = await axios.post('http://0.0.0.0:8000/verify', dataToSend, {
                headers: {
                    'Content-Type': 'application/json'
                }
            });

            if (response.status !== 200) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }

            setServerResponse(JSON.stringify(response.data));
        } catch (error) {
            console.error('Error:', error);
        } finally {
            setLoading(false);
        }
    };

    return (
        <Container maxWidth="sm">
            <Typography variant="h4" component="h1" gutterBottom>
                ZaaS (Zeth As A Service)
            </Typography>
            <FormControlLabel
                control={<Checkbox name="submit_to_bonsai" onChange={handleChange} />}
                label="Submit to Bonsai"
            />
            <FormControlLabel
                control={<Checkbox checked={showCache} onChange={() => setShowCache(!showCache)} />}
                label="Use Cache"
            />
            <FormControlLabel
                control={<Checkbox checked={showLocalExec} onChange={() => setShowLocalExec(!showLocalExec)} />}
                label="Local Execution"
            />
            <FormControlLabel
                control={<Checkbox checked={showReceiptUUID} onChange={() => setShowReceiptUUID(!showReceiptUUID)} />}
                label="Verify Bonsai Receipt UUID"
            />
            <form onSubmit={handleSubmit}>
                <TextField variant="filled" label="RPC URL" name="rpc_url" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                {showCache && <TextField variant="filled" label="Cache" name="cache" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />}
                <TextField variant="filled" label="Network" name="network" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                <TextField variant="filled" label="Block Number" name="block_no" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />
                {showLocalExec && <TextField variant="filled" label="Local Execution" name="local_exec" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />}
                {showReceiptUUID && <TextField variant="filled" label="Verify Bonsai Receipt UUID" name="verify_bonsai_receipt_uuid" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white' }} />}
                <Button type="submit" variant="contained" color="primary" disabled={loading}>
                    {loading ? <CircularProgress size={24} /> : 'Submit'}
                </Button>
            </form>
            {serverResponse && <Typography variant="body1">{serverResponse}</Typography>}
        </Container>
    );
}

export default VerificationForm;