
import React, { useState, ChangeEvent, FormEvent, useEffect } from 'react';
import { TextField, Button, FormControlLabel, Checkbox, Container, Typography, CircularProgress, Box, MenuItem, Card, CardContent, Divider } from '@mui/material';
import Websocket from 'react-websocket';

interface FormData {
    cache?: string;
    network: string;
    block_no: number;
    local_exec?: number;
    submit_to_bonsai: boolean;
    verify_bonsai_receipt_uuid?: string;
}

const VerificationForm: React.FC = () => {
    const [formData, setFormData] = useState<FormData>({
        network: '',
        block_no: 0,
        submit_to_bonsai: false,
        local_exec: 0,
        cache: '',
        verify_bonsai_receipt_uuid: '',
    });

    const [loading, setLoading] = useState(false);
    const [showCache, setShowCache] = useState(false);
    const [showLocalExec, setShowLocalExec] = useState(false);
    const [showReceiptUUID, setShowReceiptUUID] = useState(false);
    const [serverResponses, setServerResponses] = useState<string[]>([]);

    const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
        const value = event.target.type === 'checkbox' ? event.target.checked : event.target.value;
        setFormData({
            ...formData,
            [event.target.name]: (event.target.name === 'block_no' || event.target.name === 'local_exec') && typeof value === 'string' ? parseInt(value) : value
        });
    };

    const [wsData, setWsData] = useState<string | null>(null);

    const handleData = (data: string) => {
        setWsData(data);
    };

    useEffect(() => {
        if (wsData) {
            setServerResponses(prevResponses => [...prevResponses, wsData]);
        }
    }, [wsData]);

    const handleSubmit = (event: FormEvent) => {
        event.preventDefault();
        setLoading(true);
        setServerResponses([]);
        const dataToSend = { ...formData };
        if (!showCache) delete dataToSend.cache;
        if (!showLocalExec) delete dataToSend.local_exec;
        if (!showReceiptUUID) delete dataToSend.verify_bonsai_receipt_uuid;

        const isLocalEnvironment = process.env.NODE_ENV === 'development';
        const endpoint = isLocalEnvironment ? 'ws://localhost:8000/ws/verify' : 'wss://net-lb-bd44876-1703206818.us-east-1.elb.amazonaws.com:8000/ws/verify';
        console.log(endpoint);

        const socket = new WebSocket(endpoint);

        socket.onopen = function (event) {
            socket.send(JSON.stringify(dataToSend));
        };

        socket.onmessage = function (event) {
            console.log('Server response', event.data);
            setServerResponses(prevResponses => [...prevResponses, event.data]);
            setLoading(false);
        };

        socket.onerror = function (error) {
            console.log('WebSocket Error: ', error);
            setLoading(false);
        };

        socket.onclose = function (event) {
            console.log('WebSocket connection closed: ', event);
        };
    };

    return (
        <Container maxWidth="sm">
            <Websocket url='ws://localhost:8000/ws/' onMessage={handleData} />
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
                {showCache && <TextField variant="filled" label="Cache" name="cache" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />}
                <TextField
                    select
                    variant="filled"
                    label="Network"
                    name="network"
                    onChange={handleChange}
                    fullWidth
                    margin="normal"
                    sx={{ bgcolor: 'white', marginBottom: '20px' }}
                >
                    <MenuItem value="Ethereum">Ethereum</MenuItem>
                    <MenuItem value="Sepolia">Sepolia</MenuItem>
                    <MenuItem value="Goerli">Goerli</MenuItem>
                </TextField>
                <TextField variant="filled" label="Block Number" name="block_no" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />
                {showLocalExec && <TextField variant="filled" label="Local Execution" name="local_exec" type="number" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />}
                {showReceiptUUID && <TextField variant="filled" label="Verify Bonsai Receipt UUID" name="verify_bonsai_receipt_uuid" onChange={handleChange} fullWidth margin="normal" sx={{ bgcolor: 'white', marginBottom: '20px' }} />}
                <Button type="submit" variant="contained" color="primary" disabled={loading} sx={{ textTransform: 'none', fontSize: '16px', padding: '10px 20px' }}>
                    {loading ? <CircularProgress size={24} /> : 'Submit'}
                </Button>
            </Box>
            {serverResponses.length > 0 && (
                <>
                    <Divider sx={{ my: 2 }} /> {/* This creates a line between the form and the card */}
                    <Card variant="outlined">
                        <CardContent>
                            {serverResponses.map((response, index) => (
                                <Typography key={index} variant="body1" sx={{ marginBottom: '20px' }}>{response}</Typography>
                            ))}
                        </CardContent>
                    </Card>
                </>
            )}
        </Container >
    );
}

export default VerificationForm;