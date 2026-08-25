*** Settings ***
Documentation       Check that one can query context source registrations with providing page and limit parameters, pagination logic shall be in place as mandated by clause 5.5.9.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Created Context Source Registrations
Test Template       Query Context Source Registration With Limit And Offset Parameters


*** Variables ***
${first_context_source_registration_payload_file_path}=     csourceRegistrations/context-source-registration-building.jsonld
${second_context_source_registration_payload_file_path}=    csourceRegistrations/context-source-registration-location.jsonld
${third_context_source_registration_payload_file_path}=     csourceRegistrations/context-source-registration-detailed-information.jsonld


*** Test Cases ***    LIMIT    OFFSET    EXPECTED_NUMBER    PREV_LINK    NEXT_LINK
037_11_01 Query Second Subscription
    [Tags]    csr-query    5_10_2
    ${1}    ${2}    ${1}    </ngsi-ld/v1/csourceRegistrations?type=Building&limit=1&offset=1>;rel="prev";type="application/ld+json"    ${EMPTY}
037_11_02 Query Last Subscription
    [Tags]    csr-query    5_10_2
    ${2}    ${2}    ${1}    </ngsi-ld/v1/csourceRegistrations?type=Building&limit=2&offset=0>;rel="prev";type="application/ld+json"    ${EMPTY}
037_11_03 Query All Subscriptions
    [Tags]    csr-query    5_10_2
    ${15}    ${0}    ${3}    ${EMPTY}    ${EMPTY}


*** Keywords ***
Query Context Source Registration With Limit And Offset Parameters
    [Documentation]    Check that one can query context source registrations with providing page and limit parameters, pagination logic shall be in place as mandated by clause 5.5.9.
    [Arguments]    ${limit}    ${offset}    ${expected_number}    ${prev_link}    ${next_link}
    ${response}=    Query Context Source Registrations
    ...    context=${ngsild_test_suite_context}
    ...    type=Building
    ...    limit=${limit}
    ...    offset=${offset}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Number Of Entities
    ...    ContextSourceRegistration
    ...    ${expected_number}
    ...    ${response.json()}
    Check Pagination Prev And Next Headers    ${prev_link}    ${next_link}    ${response.headers}

Setup Initial Context Source Registrations
    ${first_context_source_registration_id}=    Generate Random CSR Id
    ${second_context_source_registration_id}=    Generate Random CSR Id
    ${third_context_source_registration_id}=    Generate Random CSR Id
    ${first_context_source_registration_payload}=    Load Test Sample
    ...    ${first_context_source_registration_payload_file_path}
    ...    ${first_context_source_registration_id}
    ${second_context_source_registration_payload}=    Load Test Sample
    ...    ${second_context_source_registration_payload_file_path}
    ...    ${second_context_source_registration_id}
    ${third_context_source_registration_payload}=    Load Test Sample
    ...    ${third_context_source_registration_payload_file_path}
    ...    ${third_context_source_registration_id}
    ${create_response1}=    Create Context Source Registration    ${first_context_source_registration_payload}
    Check Response Status Code    201    ${create_response1.status_code}
    ${create_response2}=    Create Context Source Registration    ${second_context_source_registration_payload}
    Check Response Status Code    201    ${create_response2.status_code}
    ${create_response3}=    Create Context Source Registration    ${third_context_source_registration_payload}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Test Variable    ${first_context_source_registration_id}
    Set Test Variable    ${second_context_source_registration_id}
    Set Test Variable    ${third_context_source_registration_id}

Delete Created Context Source Registrations
    Delete Context Source Registration    ${first_context_source_registration_id}
    Delete Context Source Registration    ${second_context_source_registration_id}
    Delete Context Source Registration    ${third_context_source_registration_id}
