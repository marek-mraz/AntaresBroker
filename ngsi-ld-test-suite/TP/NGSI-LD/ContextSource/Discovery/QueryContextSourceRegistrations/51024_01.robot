*** Settings ***
Documentation       Verify 5.10.2.4 discovery filters the official 036/037
...                 TPs skip: the context source filter (csf) over Context
...                 Source Properties, the Scope query over the registration
...                 scope, and the geoquery over the registration location.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Registrations
Suite Teardown      Teardown Registrations


*** Test Cases ***
51024_01_01 Csf Filters On Context Source Properties
    [Documentation]    5.10.2.4: "the conditions specified by the context
    ...    source filter match the respective Context Source Properties".
    [Tags]    csr-query    5_10_2    since_v1.9.1
    ${response}=    Query Context Source Registrations With Return
    ...    type=Building
    ...    csf=endpoint=="http://csf-a.example.com"
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${reg_a}
    Should Not Contain    ${response.text}    ${reg_b}

51024_01_02 Scope Query Filters On The Registration Scope
    [Documentation]    5.10.2.4: "the Scope query (as mandated by clause
    ...    4.19) is matched against the scope property".
    [Tags]    csr-query    5_10_2    4_19    since_v1.9.1
    ${response}=    Query Context Source Registrations With Return
    ...    type=Building
    ...    scopeQ=/Madrid/\#
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${reg_a}
    Should Not Contain    ${response.text}    ${reg_b}

51024_01_03 Geoquery Filters On The Registration Location
    [Documentation]    5.10.2.4: "the geoquery is matched against the
    ...    GeoProperty ... specified in the Context Source Registration";
    ...    a registration without a location does not match.
    [Tags]    csr-query    5_10_2    4_10    since_v1.9.1
    ${response}=    Query Context Source Registrations With Return
    ...    type=Building
    ...    georel=near;maxDistance==2000
    ...    geometry=Point
    ...    coordinates=[8.68,49.41]
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${reg_a}
    Should Not Contain    ${response.text}    ${reg_b}


*** Keywords ***
Setup Registrations
    ${reg_a}=    Generate Random CSR Id
    ${reg_b}=    Generate Random CSR Id
    Set Suite Variable    ${reg_a}
    Set Suite Variable    ${reg_b}
    ${payload_a}=    Evaluate
    ...    {"id": "${reg_a}", "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Building"}]}], "endpoint": "http://csf-a.example.com", "scope": "/Madrid/Centro", "location": {"type": "Point", "coordinates": [8.68, 49.41]}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Context Source Registration With Return    ${payload_a}
    Check Response Status Code    201    ${response.status_code}
    ${payload_b}=    Evaluate
    ...    {"id": "${reg_b}", "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Building"}]}], "endpoint": "http://csf-b.example.com", "scope": "/Berlin", "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Context Source Registration With Return    ${payload_b}
    Check Response Status Code    201    ${response.status_code}

Teardown Registrations
    Delete Context Source Registration    ${reg_a}
    Delete Context Source Registration    ${reg_b}
