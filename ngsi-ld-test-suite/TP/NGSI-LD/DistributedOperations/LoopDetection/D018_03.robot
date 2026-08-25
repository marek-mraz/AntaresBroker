*** Settings ***
Documentation       Verify that the request contains both the Via header from the Context Broker and the Via from the Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registrations And Stop Context Source Mock Server

*** Variables ***
${entity_payload_filename}           vehicle-simple-attributes.jsonld
${registration_payload_file_path}    csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld

*** Test Cases ***
D018_03 Check Via Header Forwarding To Context Source
    [Documentation]    Verify that the request contains the Via header from both the Context Broker and the Context Source
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    additive-inclusive    5_6_6    6_3_18

    # The inclusive registration (endpoint=/broker1) forwards the create to the
    # mock; that forward must be answered so the operation returns 201 (cf.
    # sibling D018_02, which stubs the same reply). The DELETE forward below is
    # already stubbed; the POST one was missing.
    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    201
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    ${create_via_header}=    Get From Dictionary    ${payload}    Via
    Set Suite Variable    ${create_via_header}

    Set Stub Reply    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${delete_via_header}=    Convert To Dictionary    ${response}
    Should Contain    ${delete_via_header}[Via]\[value]    ${create_via_header}

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=inclusive
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Registrations And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
