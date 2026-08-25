*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker, one is able to replace the entity on the Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_replacement}                   vehicle-simple-different-attributes.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D007_01_exc Replace Entity
    [Documentation]    Check that if one requests the Context Broker to replace an entity that matches an exclusive registration, the entity is replaced on the Context Source too
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_18

    Set Stub Reply    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    ${response}=    Replace Entity
    ...    entity_id=${entity_id}
    ...    filename=${entity_replacement}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be Equal As Integers    ${stub_count}    1

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    # 4.3.6.3: the exclusive CS owns 'speed' - a local retrieve legitimately has no such key.
    # Get From Dictionary THROWS on an absent key, so the original assertion could never verify this.
    Dictionary Should Not Contain Key    ${response.json()}    speed


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

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

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
