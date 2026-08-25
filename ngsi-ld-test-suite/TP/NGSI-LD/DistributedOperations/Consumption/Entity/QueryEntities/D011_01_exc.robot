*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker and a fragment of an entity
...                 on a Context Source, if one queries the Context Broker the query gets merged with the Context Source
...                 correctly

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_speed_filename}                vehicle-speed-attribute.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D011_01_exc Query The Context Broker With Type
    [Documentation]    Check that if one queries the Context Broker for type, entity with matching type on a Context
    ...    Source gets merged correctly
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_7_2
    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_speed_filename}    ${entity_id}
    Set Stub Reply
    ...    GET
    ...    /broker1/ngsi-ld/v1/entities?type=Vehicle&id=${entity_id}&attrs=speed
    ...    200
    ...    ${serialized_entity}

    ${response}=    Query Entities    entity_types=Vehicle    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

    ${stub_count}=    Get Stub Count    GET    /broker1/ngsi-ld/v1/entities?type=Vehicle&id=${entity_id}&attrs=speed
    Should Be True    ${stub_count} > 0

    Should Have Value In Json    ${response.json()[0]}    $.speed


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
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
