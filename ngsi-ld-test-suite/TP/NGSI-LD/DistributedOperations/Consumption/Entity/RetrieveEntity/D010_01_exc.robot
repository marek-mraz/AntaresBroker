*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker and an entity only on a Context Source, a retrieval request to the Context Broker the request is forwarded correcty to the Context Source

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
D010_01_exc Query Context Broker And Retrieve Entity By Id
    [Documentation]    Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration only allows the retrieveEntity operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_7_1

    ${entity_speed}=    Load Entity    ${entity_speed_filename}    ${entity_id}
    Set Stub Reply    GET    /broker1/ngsi-ld/v1/entities/${entity_id}    200    ${entity_speed}

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

    # NGSI-LD 5.7.1.4: a Retrieve Entity is forwarded to a matching Context Source via the retrieveEntity
    # operation (GET /entities/{id}), which is exactly what this test stubs above. The previous assertion
    # checked a queryEntity URL (entities?type=Vehicle) that can never match the registered retrieveEntity
    # stub, so it could never pass. Assert the retrieveEntity stub was hit instead (see error.md).
    ${stub_count}=    Get Stub Count    GET    /broker1/ngsi-ld/v1/entities/${entity_id}
    Should Be True    ${stub_count} > 0

    # The forwarded retrieveEntity result must contain the `speed` Property provided by the Context
    # Source. (The previous line asserted the speed Property *value* dict contains the literal string
    # "speed", which it never does — see error.md.)
    Should Contain    ${response.json()}    speed
    
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
