*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker and part of an entity on a Context Source, a retrieval request to the Context Broker does not contain the exclusive attribute

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup         Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown      Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_speed_filename}                vehicle-speed-attribute.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld

*** Test Cases *** 
D010_02_exc Query Context Broker And Retrieve Entity By Id
    [Documentation]    Check that if one retrieves a fragmented entity locally, the response does not contain the exclusive attribute
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_7_1
    
    ${entity_speed}=    Load Entity    ${entity_speed_filename}    ${entity_id}
    Set Stub Reply    GET    /broker1/ngsi-ld/v1/entities/${entity_id}    200    ${entity_speed}

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    Dictionary Should Not Contain Key    ${response.json()}    speed
    
*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
        ${entity_id}=    Generate Random Vehicle Entity Id
        Set Suite Variable   ${entity_id}

        ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
        Check Response Status Code    201    ${response.status_code}

        ${registration_id}=    Generate Random CSR Id
        Set Suite Variable    ${registration_id}
        ${registration_payload}=    Prepare Context Source Registration From File
        ...    ${registration_id}
        ...    ${registration_payload_file_path}
        ...    entity_id=${entity_id}
        ...    mode=exclusive
        ...    endpoint=/broker1
        ${response}=    Create Context Source Registration With Return    ${registration_payload}
        Check Response Status Code    201    ${response.status_code}

        Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
