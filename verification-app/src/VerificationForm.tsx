// src/VerificationForm.tsx
import axios from 'axios';
import React, { useState, ChangeEvent, FormEvent } from 'react';
import { TextField, Button, FormControlLabel, Checkbox, Container, Typography, CircularProgress, Box } from '@mui/material';

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

            // TODO: Replace with DNS Name
            const response = await axios.post(`http://net-lb-bd44876-1703206818.us-east-1.elb.amazonaws.com:8000/verify`, dataToSend, {
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
            <Box component="form" onSubmit={handleSubmit} padding={2} bgcolor="background.paper" borderRadius={2}>
                <FormControlLabel
                    control={<Checkbox name="submit_to_bonsai" onChange={handleChange} />}
                    label={<Typography color="textPrimary">Submit to Bonsai</Typography>}
                />
                <FormControlLabel
                    control={<Checkbox checked={showCache} onChange={() => setShowCache(!showCache)} />}
                    label={<Typography color="textPrimary">Use Cache</Typography>}
                />
                <FormControlLabel
                    control={<Checkbox checked={showLocalExec} onChange={() => setShowLocalExec(!showLocalExec)} />}
                    label={<Typography color="textPrimary">Local Execution</Typography>}
                />
                <FormControlLabel
                    control={<Checkbox checked={showReceiptUUID} onChange={() => setShowReceiptUUID(!showReceiptUUID)} />}
                    label={<Typography color="textPrimary">Verify Bonsai Receipt UUID</Typography>}
                />
                <TextField variant="filled" label="RPC URL" name="rpc_url" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />
                {showCache && <TextField variant="filled" label="Cache" name="cache" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />}
                <TextField variant="filled" label="Network" name="network" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />
                <TextField variant="filled" label="Block Number" name="block_no" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />
                {showLocalExec && <TextField variant="filled" label="Local Execution" name="local_exec" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />}
                {showReceiptUUID && <TextField variant="filled" label="Verify Bonsai Receipt UUID" name="verify_bonsai_receipt_uuid" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />}
                <Button type="submit" variant="contained" color="primary" disabled={loading} sx={{ textTransform: 'none', fontSize: '16px', padding: '10px 20px' }}>
                    {loading ? <CircularProgress size={24} /> : 'Submit'}
                </Button>
            </Box>
            {serverResponse && <Typography variant="body1">{serverResponse}</Typography>}
        </Container>
    );
}

export default VerificationForm;