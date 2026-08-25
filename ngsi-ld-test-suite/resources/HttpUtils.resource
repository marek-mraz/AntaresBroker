*** Settings ***
Variables       ./variables.py
Library         Collections


*** Keywords ***
Fetch Id From Response Location Header
    [Arguments]    ${response_headers}
    Dictionary Should Contain Key    ${response_headers}    Location    msg=HTTP Headers do not contain key 'Location'
    ${id}=    Get From Dictionary    ${response_headers}    Location
    RETURN    ${id}
