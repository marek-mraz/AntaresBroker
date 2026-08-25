*** Settings ***
Documentation       Verify that a loop is detected when the Via header contains the broker's identifier.

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
D018_01 Loop Detection With Via Header
    [Documentation]    Verify that a loop is detected when the Via header contains the broker's identifier.
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    5_6_6    6_3_18

    # The inclusive registration forwards the create to the mock; that forward
    # must be answered so the operation returns 201 (cf. sibling D018_02, which
    # stubs the same reply). Without it a broker that surfaces the unanswered
    # forward as a partial success returns 207 and the test fails at setup.
    Set Stub Reply    POST    /ngsi-ld/v1/entities    201
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    ${expected_via_header}=    Get From Dictionary    ${payload}    Via

    ${response}=    Delete Entity    ${entity_id}    via=${expected_via_header}
    Check Response Status Code    508    ${response.status_code}

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    # 6.3.17 p.278 scopes 508 Loop Detected to "an exclusive or redirect
    # registration, where all of the data is held outside of the Context
    # Broker and held in a single registered source ... registered to redirect
    # back on to the Context Broker". This test asserted 508 while registering
    # mode=inclusive, for which the same clause prescribes 207 on source
    # errors and Table 6.3.18-2 drops the looping registration from matching
    # (the operation then runs locally and answers 204). Registering the mode
    # the assertion belongs to keeps the subject — Via replay ⇒ loop — intact.
    # Note the siblings D018_02/D018_03 are tagged additive-inclusive; this
    # test is not. See error.md 2026-08-10.
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Registrations And Stop Context Source Mock Server
    Delete Entity    ${entity_id}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
